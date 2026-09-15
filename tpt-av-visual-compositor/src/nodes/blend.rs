//! Blend node: composites a foreground over a background with a blend mode.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;
use tpt_av_visual_timeline::BlendMode;
use wgpu::util::DeviceExt;

/// Blends `slot 1` (foreground) over `slot 0` (background).
pub struct BlendNode {
    background: Option<Arc<wgpu::TextureView>>,
    foreground: Option<Arc<wgpu::TextureView>>,
    mode: BlendMode,
    opacity: f32,
}

impl BlendNode {
    /// A blend node for the given mode and foreground opacity.
    pub fn new(mode: BlendMode, opacity: f32) -> Self {
        BlendNode {
            background: None,
            foreground: None,
            mode,
            opacity: opacity.clamp(0.0, 1.0),
        }
    }
}

impl CompositorNode for BlendNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        base: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        let (Some(background), Some(foreground)) =
            (self.background.clone(), self.foreground.clone())
        else {
            return Err(crate::gpu::device::CompositorError::InvalidOperation(
                "blend node requires two inputs".into(),
            ));
        };
        let pipeline = ctx
            .pipelines
            .get(ctx.device, ctx.shaders, "blend", ctx.target_format)?;
        let sampler = ctx
            .device
            .create_sampler(&wgpu::SamplerDescriptor::default());
        let mut params = *base;
        params.p0 = [self.opacity, 0.0, 0.0, 0.0];
        params.mode = self.mode.as_u32();
        let uniform = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tpt-visual: blend params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_layout = pipeline.get_bind_group_layout(0);
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: blend bind group"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&background),
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
                    resource: wgpu::BindingResource::TextureView(&foreground),
                },
            ],
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: blend pass"),
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
        // Two input slots: (background, foreground). Ids are wired by the
        // graph per frame via connect(), so no static ids are declared.
        Vec::new()
    }

    fn set_input(&mut self, slot: usize, view: Arc<wgpu::TextureView>) {
        match slot {
            0 => self.background = Some(view),
            1 => self.foreground = Some(view),
            _ => {}
        }
    }
}
