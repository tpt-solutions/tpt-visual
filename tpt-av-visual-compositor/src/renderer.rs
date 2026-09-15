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
use crate::avi::AviWriter;
use crate::compositor::Compositor;
use crate::gpu::device::{CompositorError, Result};
use crate::scheduler::RenderStateHandle;
use std::collections::HashMap;
use std::io::BufWriter;
use std::path::Path;
use std::sync::Arc;
use tpt_av_visual_effects::EffectRenderer;
use tpt_av_visual_timeline::{AssetId, Session};
use tpt_av_visual_utils::Resolution;

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
        self.inner.render_pass(encoder, desc, input, output, format);
    }
}

/// Reusable offscreen render target + readback buffer for CPU pixel access.
struct OffscreenTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
}

impl OffscreenTarget {
    /// Returns the target for `resolution`, (re)creating it when missing or
    /// resized.
    fn ensure<'a>(
        option: &'a mut Option<Self>,
        device: &wgpu::Device,
        resolution: Resolution,
    ) -> &'a Self {
        let matches = option
            .as_ref()
            .is_some_and(|t| t.width == resolution.width && t.height == resolution.height);
        if !matches {
            let bytes_per_row = (resolution.width * 4).div_ceil(256) * 256;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tpt-visual: offscreen target"),
                size: wgpu::Extent3d {
                    width: resolution.width,
                    height: resolution.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tpt-visual: offscreen readback"),
                size: u64::from(bytes_per_row * resolution.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            *option = Some(OffscreenTarget {
                texture,
                view,
                readback,
                width: resolution.width,
                height: resolution.height,
                bytes_per_row,
            });
        }
        option.as_ref().expect("just ensured")
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
    /// Reusable offscreen target for CPU pixel access.
    offscreen: Option<OffscreenTarget>,
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
            offscreen: None,
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

    /// Attaches the default decoder for every session asset that does not
    /// have one yet: `.mp4`/`.mov` files decode via `tpt-kinetix` (default
    /// `kinetix` feature), everything else gets a procedural test-pattern
    /// source so demos always render.
    ///
    /// Call again after publishing a session with new assets; explicitly
    /// attached decoders are never replaced.
    pub fn attach_default_decoders(&mut self) -> Result<()> {
        let snapshot = self.state.load();
        for asset in snapshot.assets.values() {
            if self.assets.contains_key(&asset.id) {
                continue;
            }
            let decoder = default_decoder(asset)?;
            self.assets
                .insert(asset.id, VideoAssetCache::new(asset.clone(), decoder));
        }
        Ok(())
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

    /// Renders the current playhead frame offscreen and returns the packed
    /// RGBA8 pixels, then advances the playhead by one frame.
    ///
    /// This is the simplest way to get pixels out of the engine — useful for
    /// tests, screenshots, thumbnails, and software pipelines. For 60 fps
    /// playback prefer `render_frame_into` with a surface (readback costs a
    /// GPU→CPU copy per frame).
    pub fn render_frame_rgba(&mut self) -> Result<Vec<u8>> {
        // Align the engine with the (possibly just-published) session before
        // sizing the offscreen target.
        let snapshot = self.state.load();
        self.compositor
            .configure(snapshot.frame_rate, snapshot.resolution);
        let resolution = self.compositor.resolution;
        let device = self.compositor.gpu.device().clone();
        let queue = self.compositor.gpu.queue().clone();
        // Take the target out so `render_frame` can borrow `self` mutably.
        let mut taken = self.offscreen.take();
        let target = OffscreenTarget::ensure(&mut taken, &device, resolution);

        self.render_frame(&target.view)?;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &target.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(target.bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: resolution.width,
                height: resolution.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        let slice = target.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| CompositorError::Gpu(e.to_string()))?
            .map_err(|e| CompositorError::Gpu(e.to_string()))?;

        let mapped = slice.get_mapped_range();
        let row_bytes = (resolution.width * 4) as usize;
        let mut out = Vec::with_capacity(row_bytes * resolution.height as usize);
        for row in 0..resolution.height {
            let start = (row * target.bytes_per_row) as usize;
            out.extend_from_slice(&mapped[start..start + row_bytes]);
        }
        drop(mapped);
        target.readback.unmap();
        self.offscreen = taken;
        Ok(out)
    }

    /// Renders `frames` frames from the current playhead into an MJPEG AVI
    /// video file and returns the number of bytes written.
    ///
    /// The one-call headless export path: attach decoders, then call this.
    pub fn render_frames_to_avi(
        &mut self,
        path: impl AsRef<Path>,
        frames: u64,
        jpeg_quality: u8,
    ) -> Result<u64> {
        // Align the engine with the session before sizing the muxer.
        let snapshot = self.state.load();
        self.compositor
            .configure(snapshot.frame_rate, snapshot.resolution);
        let resolution = self.compositor.resolution;
        let fps = (self.compositor.frame_rate.as_f32().round() as u32).max(1);

        let file = std::fs::File::create(path)?;
        let mut writer = AviWriter::new(
            BufWriter::new(file),
            resolution.width,
            resolution.height,
            fps,
        );

        for _ in 0..frames {
            let rgba = self.render_frame_rgba()?;

            writer
                .add_rgba(&rgba, resolution.width, resolution.height, jpeg_quality)
                .map_err(CompositorError::Io)?;
        }
        writer.finish().map_err(CompositorError::Io)
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
        self.compositor
            .configure(snapshot.frame_rate, snapshot.resolution);

        // 2–3. Fetch + upload frame textures for every active clip.
        let yuv_pipeline = self.compositor.yuv_pipeline()?;
        let gpu = self.compositor.gpu.clone();
        let mut uploaded: Vec<(AssetId, Arc<wgpu::TextureView>)> = Vec::new();
        {
            let mut encoder =
                gpu.device()
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
                let view = texture
                    .texture()
                    .create_view(&wgpu::TextureViewDescriptor::default());
                uploaded.push((clip.asset_id, Arc::new(view)));
            }
            gpu.queue().submit(Some(encoder.finish()));
        }
        // 4–5. Build the compositing graph and execute it.
        self.compositor.render_clips(
            &clips,
            &snapshot,
            &uploaded,
            output,
            format,
            self.playhead_frame,
        )?;

        // 6. Advance the playhead.
        self.playhead_frame += 1;
        Ok(())
    }
}

/// The default [`FrameDecoder`] for an asset path.
///
/// `.mp4`/`.mov` files decode via `tpt-kinetix` (default `kinetix`
/// feature); anything else — including the `procedural://` pseudo-protocol —
/// falls back to a procedural test-pattern source so demos always render.
///
/// # Errors
/// Returns [`CompositorError::Io`] when an MP4 path is given but the file
/// cannot be read.
pub fn default_decoder(
    asset: &tpt_av_visual_timeline::VideoAsset,
) -> Result<Box<dyn FrameDecoder>> {
    let path = asset.file_path.to_string_lossy();
    let is_media_file =
        (path.ends_with(".mp4") || path.ends_with(".mov")) && !path.starts_with("procedural://");

    #[cfg(feature = "kinetix")]
    if is_media_file {
        let bytes = std::fs::read(&asset.file_path)?;
        return Ok(Box::new(crate::assets::KinetixDecoder::from_bytes(
            bytes,
            asset.frame_rate,
            asset.resolution,
        )));
    }

    #[cfg(not(feature = "kinetix"))]
    if is_media_file {
        log::warn!("{path}: built without `kinetix`; using procedural fallback");
    }

    Ok(Box::new(crate::assets::ProceduralDecoder::new(
        asset.frame_rate,
        asset.resolution,
    )))
}
