//! The `Compositor`: the main video compositing engine.

use crate::gpu::device::{CompositorError, Result};
use crate::gpu::pipeline::{NodeParams, PipelineCache};
use crate::gpu::shader::ShaderRegistry;
use crate::gpu::texture::TexturePool;
use crate::graph::CompositorGraph;
use crate::nodes::{BlendNode, CanvasNode, EffectNode, OutputNode, SourceNode, TransformNode};
use crate::renderer::FrameEffectRunner;
use std::sync::Arc;
use tpt_av_visual_effects::EffectRenderer;
use tpt_av_visual_timeline::{AssetId, BlendMode, Clip, Session};
use tpt_av_visual_utils::{FrameRate, Resolution};

/// The main video compositing engine.
///
/// Owns the GPU context and shared render resources; the
/// [`crate::renderer::TimelineRenderer`] drives it once per frame with the
/// active clips.
pub struct Compositor {
    /// GPU device and queue.
    pub(crate) gpu: crate::gpu::device::GpuContext,
    /// The compositing graph (rebuilt per frame from the timeline).
    pub(crate) graph: CompositorGraph,
    /// Session frame rate.
    pub frame_rate: FrameRate,
    /// Canvas resolution.
    pub resolution: Resolution,
    /// Shared texture pool (`texture_cache`): recycled render targets.
    pub texture_cache: TexturePool,
    pub(crate) pipelines: PipelineCache,
    pub(crate) shaders: ShaderRegistry,
    pub(crate) effects: FrameEffectRunner,
}

impl Compositor {
    /// Creates a compositor bound to `gpu`.
    #[must_use]
    pub fn new(gpu: crate::gpu::device::GpuContext) -> Self {
        let effects = EffectRenderer::new(gpu.device().clone(), gpu.queue().clone());
        Compositor {
            gpu,
            graph: CompositorGraph::new(),
            frame_rate: FrameRate::film(),
            resolution: Resolution::full_hd(),
            texture_cache: TexturePool::new(),
            pipelines: PipelineCache::new(),
            shaders: ShaderRegistry::new(),
            effects: FrameEffectRunner::new(effects),
        }
    }

    /// The shared GPU context (for hosts that need surfaces or readback).
    #[must_use]
    pub fn gpu(&self) -> &crate::gpu::device::GpuContext {
        &self.gpu
    }

    /// Sets the session parameters (frame rate and canvas resolution).
    pub fn configure(&mut self, frame_rate: FrameRate, resolution: Resolution) {
        self.frame_rate = frame_rate;
        self.resolution = resolution;
    }

    /// Builds (or fetches) the YUV→RGBA conversion pipeline.
    pub fn yuv_pipeline(&mut self) -> Result<Arc<wgpu::RenderPipeline>> {
        self.pipelines.get_yuv(self.gpu.device(), &mut self.shaders)
    }

    /// Builds a compositing graph from the active clips and executes it into
    /// `output`.
    ///
    /// Graph shape per frame (bottom-to-top clip order):
    ///
    /// ```text
    /// canvas ──▶ blend₁ ◀── transform₁ ◀── effects₁ ◀── source₁
    ///       └─▶ blend₂ ◀── transform₂ ◀── effects₂ ◀── source₂
    ///             ...
    ///                └─▶ output ──▶ final target
    /// ```
    ///
    /// Clip opacity, track opacity, and keyframed properties are resolved
    /// here; each clip's effect chain is instantiated from its
    /// `EffectInstance` list (with keyframed effect parameters applied).
    #[allow(clippy::too_many_arguments)]
    pub fn render_clips(
        &mut self,
        clips: &[&Clip],
        session: &Session,
        uploaded: &[(AssetId, Arc<wgpu::TextureView>)],
        output: &wgpu::TextureView,
        target_format: wgpu::TextureFormat,
        frame: u64,
    ) -> Result<()> {
        let mut encoder = self
            .gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tpt-visual: composite"),
            });

        let effects_runner = &mut self.effects;

        let mut graph = CompositorGraph::new();
        let mut node_frame = crate::node::NodeFrame {
            device: self.gpu.device(),
            queue: self.gpu.queue(),
            encoder: &mut encoder,
            pipelines: &mut self.pipelines,
            shaders: &mut self.shaders,
            pool: &mut self.texture_cache,
            resolution: self.resolution,
            target_format,
            effects: effects_runner,
        };

        // Canvas root (transparent; sources composite onto it).
        let mut prev = graph.add_node(Box::new(CanvasNode::new()), 0);

        for clip in clips {
            let Some((_, view)) = uploaded.iter().find(|(id, _)| *id == clip.asset_id) else {
                continue;
            };

            // Source node forwards the uploaded frame view.
            let mut source = SourceNode::new(clip.asset_id);
            source.set_view(view.clone());
            let source_id = graph.add_node(Box::new(source), 0);

            // Effect chain (skipped when empty; the source passes through).
            let mut tail = source_id;
            if !clip.effects.is_empty() {
                let resolved = resolve_effects(clip, frame);
                if !resolved.is_empty() {
                    tail = graph.add_node(Box::new(EffectNode::new(resolved)), 1);
                    graph.connect(source_id, tail, 0)?;
                }
            }

            // Transform.
            let asset = session.assets.get(&clip.asset_id);
            let clip_size = asset
                .map(|a| (a.resolution.width as f32, a.resolution.height as f32))
                .unwrap_or((
                    self.resolution.width as f32,
                    self.resolution.height as f32,
                ));
            let (transform, opacity) = clip.effective_state_at(frame);
            let transform_id = graph.add_node(
                Box::new(TransformNode::new(transform, clip_size)),
                1,
            );
            graph.connect(tail, transform_id, 0)?;

            // Blend onto the running canvas. Track opacity multiplies the
            // clip opacity when the clip's track contributes it.
            let track_opacity = session
                .tracks
                .iter()
                .find(|t| t.clips.iter().any(|c| c.id == clip.id))
                .map_or(1.0, |t| t.opacity);
            let blend_id = graph.add_node(
                Box::new(BlendNode::new(clip.blend_mode, opacity * track_opacity)),
                2,
            );
            graph.connect(prev, blend_id, 0)?;
            graph.connect(transform_id, blend_id, 1)?;
            prev = blend_id;
        }

        let output_id = graph.add_node(Box::new(OutputNode::new()), 1);
        graph.connect(prev, output_id, 0)?;

        if std::env::var("TPT_DEBUG").is_ok() {
            eprintln!("render_clips: {} clips, final target view", clips.len());
        }
        let mut params = NodeParams::new(self.resolution.width, self.resolution.height);
        // BGRA targets need an R/B swap in the final blit.
        if matches!(
            target_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            params.mode = 1;
        }
        graph.execute(&mut node_frame, output, &params, frame)?;

        self.gpu.queue().submit(Some(encoder.finish()));
        self.graph = graph;
        Ok(())
    }
}

/// Instantiates a clip's effect chain, applying keyframed effect
/// parameters (`effects.<index>.<param>` tracks override the static bag).
fn resolve_effects(clip: &Clip, frame: u64) -> Vec<Box<dyn tpt_av_visual_effects::Effect>> {
    let mut out = Vec::new();
    for (index, instance) in clip.effects.iter().enumerate() {
        let mut params = instance.parameters.clone();
        for (key, value) in params.iter_mut() {
            let property = format!("effects.{index}.{key}");
            if let Some(track) = clip
                .keyframes
                .iter()
                .find(|k| k.property == property)
            {
                *value = track.evaluate(frame);
            }
        }
        match tpt_av_visual_effects::build_effect(&instance.effect_name, &params) {
            Ok(effect) => out.push(effect),
            Err(e) => {
                log::warn!("skipping effect {:?}: {e}", instance.effect_name);
            }
        }
    }
    out
}

/// Ensures the blend mode import is used even if the fold changes.
#[allow(dead_code)]
fn _blend_mode_roundtrip(mode: BlendMode) -> u32 {
    mode.as_u32()
}

#[allow(dead_code)]
fn _gpu_error(msg: &str) -> CompositorError {
    CompositorError::Gpu(msg.into())
}
