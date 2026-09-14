//! Output node: blits the composed frame into the final render target.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// The graph's terminal node; copies its input into the output target.
pub struct OutputNode {
    input: Option<Arc<wgpu::TextureView>>,
}

impl OutputNode {
    /// Creates the output node.
    pub fn new() -> Self {
        OutputNode { input: None }
    }
}

impl Default for OutputNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositorNode for OutputNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        base: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        let Some(input) = self.input.clone() else {
            return Err(crate::gpu::device::CompositorError::InvalidOperation(
                "output node has no input".into(),
            ));
        };
        let pipeline = ctx.pipelines.get(ctx.device, ctx.shaders, "blit", ctx.target_format)?;
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor::default());
        let uniform = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tpt-visual: output params"),
                contents: bytemuck::bytes_of(base),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_layout = pipeline.get_bind_group_layout(0);
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: output bind group"),
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
        if std::env::var("TPT_DEBUG").is_ok() {
            eprintln!("output pass begins");
        }
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: output pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        if slot == 0 {
            self.input = Some(view);
        }
    }
}
