//! Resolution and frame rate types.

use serde::{Deserialize, Serialize};

use crate::{Result, VisualError};

/// Frame dimensions in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Resolution {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Resolution {
    /// Creates a new resolution, rejecting zero dimensions.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(VisualError::InvalidOperation(
                "resolution must be non-zero".into(),
            ));
        }
        Ok(Self { width, height })
    }

    /// Total pixel count.
    #[must_use]
    pub fn pixel_count(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Aspect ratio as `width / height` (widescreen ≈ 1.7778).
    #[must_use]
    pub fn aspect_ratio(self) -> f32 {
        self.width as f32 / self.height as f32
    }

    /// Whether this resolution can contain (is at least as large as) another.
    #[must_use]
    pub fn contains(self, other: Resolution) -> bool {
        self.width >= other.width && self.height >= other.height
    }

    /// Scales this resolution to fit inside `bounds`, preserving aspect ratio
    /// and never upscaling. Result is clamped to at least 1x1.
    #[must_use]
    pub fn scale_to_fit(self, bounds: Resolution) -> Resolution {
        let sx = bounds.width as f32 / self.width as f32;
        let sy = bounds.height as f32 / self.height as f32;
        let s = sx.min(sy).min(1.0);
        Resolution {
            width: ((self.width as f32 * s).floor() as u32).max(1),
            height: ((self.height as f32 * s).floor() as u32).max(1),
        }
    }

    /// 1920x1080.
    #[must_use]
    pub fn full_hd() -> Self {
        Resolution {
            width: 1920,
            height: 1080,
        }
    }

    /// 3840x2160.
    #[must_use]
    pub fn uhd_4k() -> Self {
        Resolution {
            width: 3840,
            height: 2160,
        }
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Rational frame rate (`num / den` ticks per second), e.g. 30000/1001 for
/// 29.97 fps NTSC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRate {
    /// Numerator.
    pub num: u32,
    /// Denominator (never zero).
    pub den: u32,
}

impl FrameRate {
    /// Creates a frame rate, rejecting a zero denominator.
    pub fn new(num: u32, den: u32) -> Result<Self> {
        if den == 0 || num == 0 {
            return Err(VisualError::InvalidOperation(
                "frame rate must be non-zero".into(),
            ));
        }
        Ok(Self { num, den })
    }

    /// Exact frame rate (e.g. 24).
    pub fn exact(fps: u32) -> Result<Self> {
        Self::new(fps, 1)
    }

    /// 30000/1001 NTSC.
    #[must_use]
    pub fn ntsc() -> Self {
        FrameRate { num: 30000, den: 1001 }
    }

    /// 24 fps.
    #[must_use]
    pub fn film() -> Self {
        FrameRate { num: 24, den: 1 }
    }

    /// Frame rate as f32.
    #[must_use]
    pub fn as_f32(self) -> f32 {
        self.num as f32 / self.den as f32
    }

    /// Frame rate as f64.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// Duration of a single frame in seconds.
    #[must_use]
    pub fn frame_duration_secs(self) -> f64 {
        self.as_f64().recip()
    }

    /// Number of whole frames spanning `seconds` (rounded to nearest).
    #[must_use]
    pub fn frames_for_duration(self, seconds: f64) -> u64 {
        ((seconds * self.as_f64()).round()) as u64
    }

    /// Time in seconds at which frame `frame` starts.
    #[must_use]
    pub fn time_of_frame(self, frame: u64) -> f64 {
        frame as f64 / self.as_f64()
    }
}

impl std::fmt::Display for FrameRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.den == 1 {
            write!(f, "{} fps", self.num)
        } else {
            write!(f, "{}/{} fps", self.num, self.den)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero() {
        assert!(Resolution::new(0, 100).is_err());
        assert!(Resolution::new(100, 0).is_err());
        assert!(FrameRate::new(24, 0).is_err());
        assert!(FrameRate::exact(0).is_err());
    }

    #[test]
    fn aspect_and_contains() {
        let hd = Resolution::full_hd();
        assert!((hd.aspect_ratio() - 16.0 / 9.0).abs() < 1e-6);
        assert!(hd.contains(Resolution::new(1920, 1080).unwrap()));
        assert!(!hd.contains(Resolution::uhd_4k()));
        assert_eq!(hd.pixel_count(), 2_073_600);
    }

    #[test]
    fn scale_to_fit_preserves_aspect_never_upscales() {
        let hd = Resolution::full_hd();
        // Fits inside 4K unchanged.
        assert_eq!(hd.scale_to_fit(Resolution::uhd_4k()), hd);
        // 4K inside HD scales down to 1080p.
        assert_eq!(
            Resolution::uhd_4k().scale_to_fit(hd),
            Resolution::new(1920, 1080).unwrap()
        );
        // Portrait video in a landscape frame is height-limited.
        let portrait = Resolution::new(1080, 1920).unwrap();
        assert_eq!(
            portrait.scale_to_fit(hd),
            Resolution::new(607, 1080).unwrap()
        );
    }

    #[test]
    fn frame_rate_math() {
        let ntsc = FrameRate::ntsc();
        assert!((ntsc.as_f64() - 29.970_029_970).abs() < 1e-9);
        // One second of NTSC video is 30000/1001 * 1 ≈ 30 frames.
        assert_eq!(ntsc.frames_for_duration(1.0), 30);
        let film = FrameRate::film();
        assert_eq!(film.frames_for_duration(2.5), 60);
        assert!((film.time_of_frame(48) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn display() {
        assert_eq!(FrameRate::film().to_string(), "24 fps");
        assert_eq!(FrameRate::ntsc().to_string(), "30000/1001 fps");
        assert_eq!(Resolution::uhd_4k().to_string(), "3840x2160");
    }
}
