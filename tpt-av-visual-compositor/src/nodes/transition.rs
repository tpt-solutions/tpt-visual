//! Transition node: crossfade, wipe, dissolve with configurable curves.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{CompositorNode, NodeFrame, NodeId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Transition kinds (shader `mode` values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransitionKind {
    /// Linear blend from A to B.
    #[default]
    Crossfade,
    /// Left-to-right wipe with a soft edge.
    Wipe,
    /// Per-pixel noise dissolve.
    Dissolve,
}

/// Easing curve applied to the transition progress (Phase 4: configurable
/// transition curves).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TransitionCurve {
    /// Constant speed.
    Linear,
    /// Smooth (cubic) ease-in-out.
    Smooth,
    /// CSS-style cubic bezier `(x1, y1, x2, y2)`.
    Bezier(f32, f32, f32, f32),
}

impl Default for TransitionCurve {
    fn default() -> Self {
        TransitionCurve::Smooth
    }
}

impl TransitionCurve {
    /// Eases a raw progress value.
    #[must_use]
    pub fn ease(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            TransitionCurve::Linear => t,
            TransitionCurve::Smooth => t * t * (3.0 - 2.0 * t),
            TransitionCurve::Bezier(x1, y1, x2, y2) => cubic_bezier_ease(t, *x1, *y1, *x2, *y2),
        }
    }
}

/// Solves a CSS-style cubic bezier easing.
#[must_use]
pub fn cubic_bezier_ease(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    fn component(t: f32, p1: f32, p2: f32) -> f32 {
        let omt = 1.0 - t;
        3.0 * omt * omt * t * p1 + 3.0 * omt * t * t * p2 + t * t * t
    }
    let mut t = x;
    for _ in 0..8 {
        let diff = component(t, x1, x2) - x;
        if diff.abs() < 1e-6 {
            break;
        }
        let d = 3.0 * (1.0 - t) * (1.0 - t) * x1
            + 6.0 * (1.0 - t) * t * (x2 - x1)
            + 3.0 * t * t * (1.0 - x2);
        if d.abs() < 1e-6 {
            break;
        }
        t = (t - diff / d).clamp(0.0, 1.0);
    }
    if (component(t, x1, x2) - x).abs() > 1e-4 {
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            let mid = (lo + hi) * 0.5;
            if component(mid, x1, x2) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        t = (lo + hi) * 0.5;
    }
    component(t, y1, y2)
}

/// Crossfades / wipes / dissolves from `slot 0` to `slot 1`.
pub struct TransitionNode {
    kind: TransitionKind,
    curve: TransitionCurve,
    raw_progress: f32,
    wipe_softness: f32,
    from: Option<Arc<wgpu::TextureView>>,
    to: Option<Arc<wgpu::TextureView>>,
}

impl TransitionNode {
    /// A transition with raw progress in 0..1 (eased internally per frame).
    pub fn new(kind: TransitionKind, curve: TransitionCurve, raw_progress: f32) -> Self {
        TransitionNode {
            kind,
            curve,
            raw_progress: raw_progress.clamp(0.0, 1.0),
            wipe_softness: 0.05,
            from: None,
            to: None,
        }
    }

    /// Overrides the wipe softness (fraction of frame width).
    pub fn with_wipe_softness(mut self, softness: f32) -> Self {
        self.wipe_softness = softness;
        self
    }
}

impl CompositorNode for TransitionNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        base: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        let (Some(from), Some(to)) = (self.from.clone(), self.to.clone()) else {
            return Err(crate::gpu::device::CompositorError::InvalidOperation(
                "transition node requires two inputs".into(),
            ));
        };
        let pipeline = ctx
            .pipelines
            .get(ctx.device, ctx.shaders, "transition", ctx.target_format)?;
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor::default());
        let mut params = base.clone();
        params.p0 = [self.wipe_softness, 0.0, 0.0, 0.0];
        params.mode = match self.kind {
            TransitionKind::Crossfade => 0,
            TransitionKind::Wipe => 1,
            TransitionKind::Dissolve => 2,
        };
        params.progress = self.curve.ease(self.raw_progress);
        let uniform = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tpt-visual: transition params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_layout = pipeline.get_bind_group_layout(0);
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: transition bind group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&from),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&to),
                },
            ],
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: transition pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }

    fn inputs(&self) -> Vec<NodeId> {
        Vec::new()
    }

    fn set_input(&mut self, slot: usize, view: Arc<wgpu::TextureView>) {
        match slot {
            0 => self.from = Some(view),
            1 => self.to = Some(view),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_monotone_and_bounded() {
        for curve in [
            TransitionCurve::Linear,
            TransitionCurve::Smooth,
            TransitionCurve::Bezier(0.25, 0.1, 0.25, 1.0),
        ] {
            let mut prev = -0.01_f32;
            for i in 0..=20 {
                let v = curve.ease(i as f32 / 20.0);
                assert!((0.0..=1.0).contains(&v));
                assert!(v >= prev - 1e-4, "non-monotone at {i}");
                prev = v;
            }
            assert_eq!(curve.ease(0.0), 0.0);
            assert_eq!(curve.ease(1.0), 1.0);
        }
    }

    #[test]
    fn bezier_curve_matches_reference_values() {
        // CSS ease anchors (same reference values as the timeline crate).
        let ease = TransitionCurve::Bezier(0.25, 0.1, 0.25, 1.0);
        assert!((ease.ease(0.1) - 0.0937).abs() < 5e-3);
        assert!((ease.ease(0.5) - 0.8024).abs() < 5e-3);
    }
}
