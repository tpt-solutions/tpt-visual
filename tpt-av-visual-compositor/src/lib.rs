//! `tpt-av-visual-compositor` — the GPU-accelerated rendering engine of the
//! TPT AV visual stack.
//!
//! The engine reads timeline state and renders video frames:
//!
//! - [`scheduler::RenderStateHandle`] publishes lock-free timeline
//!   snapshots from the UI thread to the render thread.
//! - [`renderer::TimelineRenderer`] implements the six-step frame pipeline:
//!   snapshot → fetch frames → upload → build graph → execute → advance.
//! - [`graph::CompositorGraph`] runs [`node::CompositorNode`]s in
//!   topological order: [`nodes::SourceNode`] presents uploaded frames,
//!   [`nodes::EffectNode`] applies effect chains, [`nodes::TransformNode`]
//!   positions them, [`nodes::BlendNode`] composites them over the running
//!   canvas, and [`nodes::OutputNode`] blits the final frame.
//! - [`assets::VideoAssetCache`] decodes and caches frames (with background
//!   prefetching, LRU GPU residency, and proxy support), powered by
//!   [`assets::FrameDecoder`] implementations — including real MP4/H.264
//!   decoding via `tpt-kinetix` behind the `kinetix` feature.

pub mod assets;
pub mod avi;
pub mod compositor;
pub mod gpu;
pub mod graph;
pub mod node;
pub mod nodes;
pub mod renderer;
pub mod scheduler;

#[cfg(feature = "kinetix")]
pub use assets::KinetixDecoder;
pub use assets::{
    FrameDecoder, ImageSequenceDecoder, ProceduralDecoder, ProxyConfig, SolidDecoder,
    VideoAssetCache,
};
pub use compositor::Compositor;
pub use gpu::device::{CompositorError, GpuContext, GpuInfo, Result};
pub use graph::CompositorGraph;
pub use node::{CompositorNode, NodeId};
pub use nodes::{
    BlendNode, CanvasNode, ChromaKeyParams, EffectNode, MaskNode, OutputNode, SourceNode,
    TransformNode, TransitionCurve, TransitionKind, TransitionNode,
};
pub use renderer::{default_decoder, TimelineRenderer};
pub use scheduler::RenderStateHandle;
