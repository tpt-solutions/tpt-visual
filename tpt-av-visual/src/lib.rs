//! `tpt-av-visual` — the front door of the TPT AV visual stack.
//!
//! One dependency that re-exports the whole stack plus a few batteries-
//! included helpers so the common path stays short:
//!
//! ```no_run
//! use tpt_av_visual::prelude::*;
//!
//! // 1. Describe the edit (fluent builder or plain structs).
//! let session = SessionBuilder::new("Demo", FrameRate::film(), Resolution::full_hd())
//!     .add_video_asset_with(
//!         "procedural://intro",
//!         240,
//!         FrameRate::film(),
//!         Resolution::full_hd(),
//!         PixelFormat::Rgba8,
//!         "Rec709",
//!     )
//!     .add_clip(ClipSpec::new(0, 0, 0).duration(240))
//!     .build()
//!     .expect("valid edit");
//!
//! // 2. Probe the GPU (Vulkan/Metal/D3D12 via wgpu).
//! println!("GPU: {:?}", tpt_av_visual::probe_gpu());
//!
//! // 3. Render headlessly to an MJPEG AVI (needs a GPU).
//! let mut renderer = TimelineRenderer::headless(session)
//!     .expect("engine error")
//!     .expect("no GPU adapter available");
//! renderer.attach_default_decoders()?;
//! renderer.render_frames_to_avi("demo.avi", 240, 90)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Crate map
//!
//! | Module | Crate | Purpose |
//! |--------|-------|---------|
//! | [`utils`] | `tpt-av-visual-utils` | Frames, pixel formats, resolution, timecode. |
//! | [`timeline`] | `tpt-av-visual-timeline` | Non-destructive edit model, edits, undo/redo. |
//! | [`compositor`] | `tpt-av-visual-compositor` | GPU compositing engine + asset caches. |
//! | [`color`] | `tpt-av-visual-color` | Color spaces, HDR, LUTs, ACES. |
//! | [`effects`] | `tpt-av-visual-effects` | Effect chain + WGSL shaders. |

pub use tpt_av_visual_color as color;
pub use tpt_av_visual_compositor as compositor;
pub use tpt_av_visual_effects as effects;
/// Non-destructive edit model (re-export of `tpt-av-visual-timeline`).
pub use tpt_av_visual_timeline as timeline;
pub use tpt_av_visual_utils as utils;

pub use tpt_av_visual_compositor::{Compositor, CompositorError, TimelineRenderer};
pub use tpt_av_visual_timeline::{BlendMode, Session, VideoAsset};
pub use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution, VisualError};

pub mod builder;
pub use builder::{ClipSpec, SessionBuilder};

/// Errors surfaced by the facade helpers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The engine failed.
    #[error(transparent)]
    Compositor(#[from] CompositorError),
    /// The timeline failed (edits, serialization).
    #[error(transparent)]
    Timeline(#[from] tpt_av_visual_timeline::TimelineError),
    /// No GPU adapter is available on this machine.
    #[error("no compatible GPU device is available")]
    NoDevice,
    /// A media file could not be read.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything a typical application needs, in one `use`.
pub mod prelude {
    pub use crate::{default_decoder, probe_gpu, ClipSpec, SessionBuilder};
    pub use tpt_av_visual_compositor::{
        FrameDecoder, ProceduralDecoder, SolidDecoder, TimelineRenderer,
    };
    pub use tpt_av_visual_timeline::{
        BlendMode, Clip, EffectInstance, History, InterpolationMethod, Keyframe, KeyframeTrack,
        Session, Track, Transform, VideoAsset,
    };
    pub use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution, Timecode, VideoFrame};
}

/// Describes the GPU the engine will use, or `None` when no adapter exists
/// (headless CI, VMs without GPU passthrough).
///
/// ```
/// match tpt_av_visual::probe_gpu() {
///     Some(gpu) => println!("rendering on {} ({})", gpu.adapter, gpu.backend),
///     None => println!("no GPU — CPU reference paths only"),
/// }
/// ```
#[must_use]
pub fn probe_gpu() -> Option<compositor::GpuInfo> {
    compositor::GpuContext::headless().map(|ctx| ctx.info())
}

/// The default [`compositor::FrameDecoder`] for an asset.
///
/// `.mp4`/`.mov` files go through `tpt-kinetix` (default `kinetix` feature);
/// anything else — including the `procedural://` pseudo-protocol — falls
/// back to a procedural test-pattern source so demos always render. Pass
/// your own decoder to [`TimelineRenderer::attach_asset`] for full control.
///
/// # Errors
/// Returns [`Error::Io`] when an MP4 path is given but the file cannot be
/// read.
pub fn default_decoder(asset: &VideoAsset) -> Result<Box<dyn compositor::FrameDecoder>> {
    let path = asset.file_path.to_string_lossy();
    let is_media_file =
        (path.ends_with(".mp4") || path.ends_with(".mov")) && !path.starts_with("procedural://");

    #[cfg(feature = "kinetix")]
    if is_media_file {
        let bytes = std::fs::read(&asset.file_path)?;
        return Ok(Box::new(compositor::KinetixDecoder::from_bytes(
            bytes,
            asset.frame_rate,
            asset.resolution,
        )));
    }

    #[cfg(not(feature = "kinetix"))]
    if is_media_file {
        log::warn!("{path}: built without `kinetix`; using procedural fallback");
    }

    Ok(Box::new(compositor::ProceduralDecoder::new(
        asset.frame_rate,
        asset.resolution,
    )))
}
