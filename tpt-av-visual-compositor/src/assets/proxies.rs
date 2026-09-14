//! Proxy generation: lower-res transcodes for smooth 4K/8K playback.
//!
//! Proxies are CPU-rescaled RGBA copies of decoded frames; the cache stores
//! them alongside the originals and `get_frame` serves proxies when enabled.
//! (GPU downscaling is on the roadmap; CPU rescale keeps proxy generation
//! independent of the render thread.)

use tpt_av_visual_utils::{PixelFormat, VideoFrame};

/// Proxy configuration for a [`crate::assets::VideoAssetCache`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProxyConfig {
    /// Target height in pixels (width follows aspect ratio).
    pub target_height: u32,
}

impl ProxyConfig {
    /// A proxy at half or quarter resolution, whichever is closer.
    #[must_use]
    pub fn for_source(height: u32) -> Option<Self> {
        let target = match height {
            0..=1_079 => return None, // HD and below: no proxy needed
            1_080..=2_159 => 540,
            _ => 1080,
        };
        Some(ProxyConfig {
            target_height: target,
        })
    }
}

/// Downscales an RGBA frame to `target_height` (aspect-preserving).
#[must_use]
pub fn generate_proxy(frame: &VideoFrame, target_height: u32) -> VideoFrame {
    let rgba = frame.to_rgba();
    let src = image::RgbaImage::from_raw(frame.width, frame.height, rgba)
        .expect("buffer matches dimensions");
    let scale = f64::from(target_height) / f64::from(frame.height);
    let new_w = ((f64::from(frame.width) * scale).round() as u32).max(1);
    let new_h = target_height.max(1);
    let resized = image::imageops::resize(
        &src,
        new_w,
        new_h,
        image::imageops::FilterType::Triangle,
    );
    VideoFrame::from_rgba(new_w, new_h, resized.into_raw(), frame.frame_number)
}

/// Whether a frame is proxied (diagnostics / tests).
#[must_use]
pub fn is_proxy(frame: &VideoFrame) -> bool {
    frame.pixel_format == PixelFormat::Rgba8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_config_targets() {
        assert!(ProxyConfig::for_source(720).is_none());
        assert_eq!(
            ProxyConfig::for_source(2160).map(|c| c.target_height),
            Some(1080)
        );
        assert_eq!(
            ProxyConfig::for_source(1080).map(|c| c.target_height),
            Some(540)
        );
    }

    #[test]
    fn generate_proxy_downscales() {
        let frame = VideoFrame::from_rgba(64, 32, vec![128; 64 * 32 * 4], 0);
        let proxy = generate_proxy(&frame, 16);
        assert_eq!(proxy.height, 16);
        assert_eq!(proxy.width, 32);
        assert_eq!(proxy.pixel_format, PixelFormat::Rgba8);
        assert_eq!(proxy.data.len(), 32 * 16 * 4);
    }
}
