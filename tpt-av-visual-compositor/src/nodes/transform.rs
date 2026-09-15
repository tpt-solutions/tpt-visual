//! Transform node: position / scale / rotation of a clip over the canvas.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{Affine2x3, CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;
use tpt_av_visual_timeline::Transform;
use wgpu::util::DeviceExt;

/// Samples its input through the inverse of the clip transform; fragments
/// outside the source footprint are transparent.
pub struct TransformNode {
    input: Option<Arc<wgpu::TextureView>>,
    transform: Transform,
    clip_size: (f32, f32),
}

impl TransformNode {
    /// A transform node for a clip of `clip_size` pixels.
    pub fn new(transform: Transform, clip_size: (f32, f32)) -> Self {
        TransformNode {
            input: None,
            transform,
            clip_size,
        }
    }

    /// Builds the target-UV → source-UV affine from a [`Transform`].
    ///
    /// The forward transform maps clip pixels onto the canvas (see
    /// `tpt-av-visual-timeline::Transform::to_matrix`); this inverse lets
    /// the shader pull-sample the source per target fragment.
    #[must_use]
    pub fn transform_to_affine(
        t: &Transform,
        clip_size: (f32, f32),
        canvas_size: (f32, f32),
    ) -> Affine2x3 {
        let m = t.to_matrix(clip_size, canvas_size);
        let inv = m
            .inverse()
            .unwrap_or(tpt_av_visual_timeline::transform::Matrix2x3::IDENTITY);
        // Canvas pixels → normalized target UV, then inverse affine to clip
        // pixels, then clip pixels → source UV.
        let sx = 1.0 / clip_size.0;
        let sy = 1.0 / clip_size.1;
        let _ux = 1.0 / canvas_size.0;
        let uy = 1.0 / canvas_size.1;
        // canvas_px = (uv.x * W, uv.y * H); compose: uv_src =
        // inv(canvas_px) * (sx, sy), translated by none (inv includes it).
        Affine2x3 {
            a: inv.a * sx,
            b: inv.b * sy,
            tx: inv.tx * sx,
            c: inv.c * uy,
            d: inv.d * uy,
            ty: inv.ty * uy,
        }
    }
}

impl CompositorNode for TransformNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        base: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        let Some(input) = self.input.clone() else {
            return Err(crate::gpu::device::CompositorError::InvalidOperation(
                "transform node has no input".into(),
            ));
        };
        let pipeline =
            ctx.pipelines
                .get(ctx.device, ctx.shaders, "transform", ctx.target_format)?;
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tpt-visual: transform sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let affine = Self::transform_to_affine(
            &self.transform,
            self.clip_size,
            (ctx.resolution.width as f32, ctx.resolution.height as f32),
        );
        let params = (*base).with_affine2x3(&affine);
        let uniform = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tpt-visual: transform params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_layout = pipeline.get_bind_group_layout(0);
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: transform bind group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
                // Layout declares a second texture slot (used by blend-family
                // shaders); single-input shaders just rebind the input.
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&input),
                },
            ],
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: transform pass"),
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
        vec![]
    }

    fn set_input(&mut self, slot: usize, view: Arc<wgpu::TextureView>) {
        if slot == 0 {
            self.input = Some(view);
        }
    }
}
