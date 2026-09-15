//! Effect node: runs a clip's effect chain through the effects crate.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;
use tpt_av_visual_effects::{Effect, EffectPassDesc};

/// Executes an effect chain (single input → output).
pub struct EffectNode {
    effects: Vec<Box<dyn Effect>>,
    input: Option<Arc<wgpu::TextureView>>,
}

impl EffectNode {
    /// Wraps a chain; an empty chain should be skipped by the renderer.
    pub fn new(effects: Vec<Box<dyn Effect>>) -> Self {
        EffectNode {
            effects,
            input: None,
        }
    }
}

impl CompositorNode for EffectNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        _params: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        let Some(input) = self.input.clone() else {
            return Err(crate::gpu::device::CompositorError::InvalidOperation(
                "effect node has no input".into(),
            ));
        };
        let (width, height) = (ctx.resolution.width, ctx.resolution.height);

        // Materialize pass descriptors (uniforms depend on target size).
        let chains: Vec<Vec<EffectPassDesc>> = self
            .effects
            .iter()
            .map(|e| e.passes(width, height))
            .collect();
        let total_passes: usize = chains.iter().map(Vec::len).sum();

        // Ping-pong through pooled scratch textures; the final pass lands
        // in `output`.
        let mut pass_index = 0_usize;
        let mut scratch: Vec<wgpu::Texture> = Vec::new();
        let mut cursor_view: Option<wgpu::TextureView> = None;

        for chain in &chains {
            for desc in chain {
                pass_index += 1;
                let last = pass_index == total_passes;
                let source: &wgpu::TextureView = cursor_view.as_ref().unwrap_or(&input);
                if last {
                    ctx.effects
                        .render_pass(ctx.encoder, desc, source, output, ctx.target_format);
                } else {
                    let texture = ctx.pool.acquire(
                        ctx.device,
                        width,
                        height,
                        wgpu::TextureUsages::RENDER_ATTACHMENT,
                    );
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    ctx.effects
                        .render_pass(ctx.encoder, desc, source, &view, ctx.target_format);
                    cursor_view = Some(view);
                    scratch.push(texture);
                }
            }
        }

        for texture in scratch {
            ctx.pool.release(texture);
        }
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
