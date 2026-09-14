//! `TimelineRenderer`: reads timeline state and renders frames.
//!
//! `render_frame` implements the six-step pipeline from the design:
//!
//! 1. Read timeline state (lock-free snapshot).
//! 2. For each active clip, fetch the video frame from the asset cache.
//! 3. Upload frames to GPU textures.
//! 4. Build the compositing graph from the timeline.
//! 5. Execute the graph.
//! 6. Advance the playhead.

use crate::assets::cache::VideoAssetCache;
use crate::assets::decoder::FrameDecoder;
use crate::compositor::Compositor;
use crate::gpu::device::{CompositorError, Result};
use crate::scheduler::RenderStateHandle;
use std::collections::HashMap;
use std::sync::Arc;
use tpt_av_visual_effects::EffectRenderer;
use tpt_av_visual_timeline::{AssetId, Session};
use tpt_av_visual_utils::VideoFrame;

/// Thin wrapper exposing the shared effect renderer to node passes.
pub struct FrameEffectRunner {
    inner: EffectRenderer,
}

impl FrameEffectRunner {
    pub(crate) fn new(inner: EffectRenderer) -> Self {
        FrameEffectRunner { inner }
    }

    /// Records one effect pass.
    pub fn render_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        desc: &tpt_av_visual_effects::EffectPassDesc,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        self.inner
            .render_pass(encoder, desc, input, output, format);
    }
}

/// Renders the timeline session through the compositor.
pub struct TimelineRenderer {
    /// Lock-free timeline snapshots (published by the UI thread).
    state: RenderStateHandle,
    /// Per-asset decode caches.
    assets: HashMap<AssetId, VideoAssetCache>,
    /// Current playhead position (in session frames).
    playhead_frame: u64,
    /// The compositor engine.
    compositor: Compositor,
}

impl TimelineRenderer {
    /// Creates a renderer for `session` on a headless GPU context.
    ///
    /// Returns `None` when no GPU adapter exists (CI without a GPU).
    pub fn headless(session: Session) -> Result<Option<Self>> {
        let Some(gpu) = crate::gpu::device::GpuContext::headless() else {
            return Ok(None);
        };
        Ok(Some(Self::new(session, gpu)?))
    }

    /// Creates a renderer on an existing GPU context.
    pub fn new(session: Session, gpu: crate::gpu::device::GpuContext) -> Result<Self> {
        let state = RenderStateHandle::new(session);
        let compositor = Compositor::new(gpu);
        Ok(TimelineRenderer {
            state,
            assets: HashMap::new(),
            playhead_frame: 0,
            compositor,
        })
    }

    /// UI thread: publishes a new timeline snapshot.
    pub fn publish_session(&self, session: Session) {
        self.state.publish(session);
    }

    /// UI thread: attaches an asset and its decoder.
    pub fn attach_asset(
        &mut self,
        asset: tpt_av_visual_timeline::VideoAsset,
        decoder: Box<dyn FrameDecoder>,
    ) {
        self.assets
            .insert(asset.id, VideoAssetCache::new(asset, decoder));
    }

    /// Mutable access to an asset cache (prefetch control, proxies).
    pub fn asset_cache_mut(&mut self, id: AssetId) -> Option<&mut VideoAssetCache> {
        self.assets.get_mut(&id)
    }

    /// The current playhead position.
    #[must_use]
    pub const fn playhead(&self) -> u64 {
        self.playhead_frame
    }

    /// Seeks the playhead.
    pub fn seek(&mut self, frame: u64) {
        self.playhead_frame = frame;
    }

    /// The compositor (for surface-aware hosts).
    pub fn compositor_mut(&mut self) -> &mut Compositor {
        &mut self.compositor
    }

    /// Prefetches `[playhead, playhead + span)` on every visible asset.
    pub fn prefetch_around_playhead(&mut self, span: u64) {
        let snapshot = self.state.load();
        let start = self.playhead_frame;
        let end = start + span;
        let active_assets: Vec<AssetId> = snapshot
            .tracks
            .iter()
            .filter(|t| t.is_visible())
            .flat_map(|t| t.clips.iter())
            .filter(|clip| clip.end_frame() > start && clip.start_frame < end)
            .map(|clip| clip.asset_id)
            .collect();
        for id in active_assets {
            if let Some(cache) = self.assets.get_mut(&id) {
                cache.prefetch(start, end);
            }
        }
    }

    /// Renders the current playhead frame into `output`, then advances the
    /// playhead by one frame. Assumes an RGBA8-unorm target.
    pub fn render_frame(&mut self, output: &wgpu::TextureView) -> Result<()> {
        self.render_frame_into(output, wgpu::TextureFormat::Rgba8Unorm)
    }

    /// [`TimelineRenderer::render_frame`] with an explicit target format
    /// (e.g. a surface's `Bgra8Unorm`, which triggers an R/B swap in the
    /// final blit).
    pub fn render_frame_into(
        &mut self,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<()> {
        // 1. Read timeline state (lock-free snapshot).
        let snapshot = self.state.load();
        let clips = snapshot.active_clips_at(self.playhead_frame);

        // 2–3. Fetch + upload frame textures for every active clip.
        let yuv_pipeline = self.compositor.yuv_pipeline()?;
        let gpu = self.compositor.gpu.clone();
        let mut uploaded: Vec<(AssetId, Arc<wgpu::TextureView>)> = Vec::new();
        {
            let mut encoder = gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tpt-visual: frame upload"),
                });
            for clip in &clips {
                let Some(cache) = self.assets.get_mut(&clip.asset_id) else {
                    continue;
                };
                let source_frame = clip.source_frame_at(self.playhead_frame);
                let texture = cache.get_frame(
                    source_frame,
                    gpu.device(),
                    gpu.queue(),
                    &mut encoder,
                    &yuv_pipeline,
                )?;
                if std::env::var("TPT_DEBUG").is_ok() {
                    eprintln!(
                        "uploaded frame {}x{} format {:?} usage {:?}",
                        texture.resolution().width,
                        texture.resolution().height,
                        texture.format(),
                        texture.texture().usage()
                    );
                }
                let view = texture.texture().create_view(&wgpu::TextureViewDescriptor::default());
                uploaded.push((clip.asset_id, Arc::new(view)));
            }
            gpu.queue().submit(Some(encoder.finish()));
        }
        // Views keep the textures alive through the graph execution below.
        let _keep_alive = &self.assets;

        // 4–5. Build the compositing graph and execute it.
        self.compositor
            .render_clips(&clips, &snapshot, &uploaded, output, format, self.playhead_frame)?;

        // 6. Advance the playhead.
        self.playhead_frame += 1;
        Ok(())
    }
}

#[allow(dead_code)]
fn _frame_type_check(frame: &VideoFrame) {
    let _ = CompositorError::InvalidOperation(format!("{}", frame.frame_number));
}
