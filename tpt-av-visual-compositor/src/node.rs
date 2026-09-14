//! The compositing node trait.

use crate::gpu::device::Result;
use crate::gpu::pipeline::{NodeParams, PipelineCache};
use crate::gpu::shader::ShaderRegistry;
use crate::gpu::texture::TexturePool;
use std::sync::Arc;
use tpt_av_visual_utils::Resolution;

/// Identifier of a node within a [`crate::graph::CompositorGraph`].
pub type NodeId = usize;

/// Row-major 2x3 affine matrix `[a b tx; c d ty]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine2x3 {
    /// Row 0, column 0.
    pub a: f32,
    /// Row 0, column 1.
    pub b: f32,
    /// Row 0 translation.
    pub tx: f32,
    /// Row 1, column 0.
    pub c: f32,
    /// Row 1, column 1.
    pub d: f32,
    /// Row 1 translation.
    pub ty: f32,
}

impl Affine2x3 {
    /// The identity transform.
    
    pub const IDENTITY: Affine2x3 = Affine2x3 {
        a: 1.0,
        b: 0.0,
        tx: 0.0,
        c: 0.0,
        d: 1.0,
        ty: 0.0,
    };
}

/// Per-frame rendering context handed to nodes.
pub struct NodeFrame<'a> {
    /// Logical device.
    pub device: &'a wgpu::Device,
    /// Command queue.
    pub queue: &'a wgpu::Queue,
    /// Encoder for this frame's commands.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// Shared pipeline cache.
    pub pipelines: &'a mut PipelineCache,
    /// Shared shader registry.
    pub shaders: &'a mut ShaderRegistry,
    /// Shared texture pool (scratch render targets).
    pub pool: &'a mut TexturePool,
    /// Canvas resolution for this render.
    pub resolution: Resolution,
    /// Pixel format of node render targets (pooled textures are
    /// Rgba8Unorm; the final target may be a surface format).
    pub target_format: wgpu::TextureFormat,
    /// Effect renderer for effect-chain nodes.
    pub effects: &'a mut effects_impl::EffectRunner,
}

/// Implementation shim so `NodeFrame` can reference the effect runner
/// without a circular module dependency.
pub mod effects_impl {
    /// Runs an effect chain (wraps `tpt-av-visual-effects::EffectRenderer`).
    pub type EffectRunner = crate::renderer::FrameEffectRunner;
}

/// A single node in the compositing graph.
///
/// Nodes are wired by the graph in topological order: `set_input` is called
/// for every declared input slot before `render`. Single-input nodes sample
/// slot 0; blend-family nodes sample slot 0 (background / from) and slot 1
/// (foreground / to).
pub trait CompositorNode: Send {
    /// Renders this node's output into `output` for timeline frame `frame`.
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        params: &NodeParams,
        frame: u64,
    ) -> Result<()>;

    /// The input nodes this node depends on, in slot order.
    fn inputs(&self) -> Vec<NodeId>;

    /// Receives a resolved input view for `slot`.
    fn set_input(&mut self, slot: usize, view: Arc<wgpu::TextureView>);

    /// Passthrough nodes (sources, optimized-out effects) forward their
    /// input view instead of rendering; the graph then skips allocation.
    /// Default: `None` (render normally).
    fn output_override(&self) -> Option<Arc<wgpu::TextureView>> {
        None
    }
}
