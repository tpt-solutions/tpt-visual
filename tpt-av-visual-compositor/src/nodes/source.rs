//! Source node: presents an uploaded video frame to the graph.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;

/// The transparent canvas root every compositing fold starts from.
pub struct CanvasNode;

impl CanvasNode {
    /// Creates the canvas node.
    #[must_use]
    pub fn new() -> Self {
        CanvasNode
    }
}

impl Default for CanvasNode {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositorNode for CanvasNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        _params: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        // Clear-only pass: the canvas starts fully transparent.
        let pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: canvas clear"),
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
        drop(pass);
        Ok(())
    }

    fn inputs(&self) -> Vec<NodeId> {
        Vec::new()
    }

    fn set_input(&mut self, _slot: usize, _view: Arc<wgpu::TextureView>) {}
}

/// A passthrough node wrapping an already-uploaded frame texture.
///
/// The renderer uploads decoded frames (step 3 of the frame pipeline) and
/// hands the texture views to source nodes when building the graph (step
/// 4); rendering is a no-op because the graph forwards the view directly.
pub struct SourceNode {
    asset_id: tpt_av_visual_timeline::AssetId,
    view: Option<Arc<wgpu::TextureView>>,
}

impl SourceNode {
    /// A source for the given asset; `set_view` must be called before
    /// execution.
    pub fn new(asset_id: tpt_av_visual_timeline::AssetId) -> Self {
        SourceNode { asset_id, view: None }
    }

    /// Supplies the uploaded frame view for this frame.
    pub fn set_view(&mut self, view: Arc<wgpu::TextureView>) {
        self.view = Some(view);
    }

    /// The asset this source draws from.
    #[must_use]
    pub fn asset_id(&self) -> tpt_av_visual_timeline::AssetId {
        self.asset_id
    }
}

impl CompositorNode for SourceNode {
    fn render(
        &mut self,
        _ctx: &mut NodeFrame,
        _output: &wgpu::TextureView,
        _params: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        Ok(())
    }

    fn inputs(&self) -> Vec<NodeId> {
        Vec::new()
    }

    fn set_input(&mut self, _slot: usize, _view: Arc<wgpu::TextureView>) {}

    fn output_override(&self) -> Option<Arc<wgpu::TextureView>> {
        self.view.clone()
    }
}
