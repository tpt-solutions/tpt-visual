//! Pixel format definitions and layout helpers.

use serde::{Deserialize, Serialize};

use crate::{Result, error::VisualError};

/// Pixel / chroma-sampling formats carried by a [`VideoFrame`](crate::frame::VideoFrame).
///
/// All planar YUV formats are 8-bit and stored as contiguous planes in a
/// single buffer in Y, U, V (then A, for `Yuv420pA`) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PixelFormat {
    /// 4:2:0 planar YUV, 8-bit (the dominant camera/broadcast format).
    Yuv420p,
    /// 4:2:2 planar YUV, 8-bit.
    Yuv422p,
    /// 4:4:4 planar YUV, 8-bit (full chroma).
    Yuv444p,
    /// 24-bit packed RGB, byte order R, G, B.
    Rgb24,
    /// 24-bit packed BGR, byte order B, G, R.
    Bgr24,
    /// 32-bit packed RGBA, byte order R, G, B, A.
    Rgba8,
}

impl PixelFormat {
    /// Number of planes for this format.
    #[must_use]
    pub fn num_planes(self) -> usize {
        match self {
            PixelFormat::Yuv420p | PixelFormat::Yuv422p | PixelFormat::Yuv444p => 3,
            PixelFormat::Rgb24 | PixelFormat::Bgr24 | PixelFormat::Rgba8 => 1,
        }
    }

    /// Average bits per pixel (packed formats count every byte).
    #[must_use]
    pub fn bits_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Yuv420p => 12,
            PixelFormat::Yuv422p => 16,
            PixelFormat::Yuv444p => 24,
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => 24,
            PixelFormat::Rgba8 => 32,
        }
    }

    /// Bytes per pixel for packed formats; `None` for planar formats.
    #[must_use]
    pub fn bytes_per_pixel(self) -> Option<usize> {
        match self {
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => Some(3),
            PixelFormat::Rgba8 => Some(4),
            _ => None,
        }
    }

    /// `true` for the planar YUV family.
    #[must_use]
    pub fn is_yuv(self) -> bool {
        matches!(
            self,
            PixelFormat::Yuv420p | PixelFormat::Yuv422p | PixelFormat::Yuv444p
        )
    }

    /// `true` if this format carries an alpha channel.
    #[must_use]
    pub fn has_alpha(self) -> bool {
        matches!(self, PixelFormat::Rgba8)
    }

    /// Dimensions of the given chroma plane (0 = Y, 1 = U, 2 = V).
    ///
    /// YUV formats subsample chroma horizontally (and vertically for 4:2:0).
    #[must_use]
    pub fn plane_dimensions(self, width: u32, height: u32, plane: usize) -> (u32, u32) {
        match (self, plane) {
            (PixelFormat::Yuv420p, 1 | 2) => ((width + 1) / 2, (height + 1) / 2),
            (PixelFormat::Yuv422p, 1 | 2) => ((width + 1) / 2, height),
            _ => (width, height),
        }
    }

    /// Total size in bytes of the given plane.
    #[must_use]
    pub fn plane_size(self, width: u32, height: u32, plane: usize) -> usize {
        let (w, h) = self.plane_dimensions(width, height, plane);
        (w as usize) * (h as usize)
    }

    /// Expected total buffer length for `width x height` frame data.
    ///
    /// Returns `None` on overflow.
    #[must_use]
    pub fn expected_data_len(self, width: u32, height: u32) -> Option<usize> {
        match self {
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => {
                (width as usize)
                    .checked_mul(height as usize)?
                    .checked_mul(3)
            }
            PixelFormat::Rgba8 => (width as usize)
                .checked_mul(height as usize)?
                .checked_mul(4),
            _ => {
                let mut total = 0usize;
                for plane in 0..self.num_planes() {
                    total = total.checked_add(self.plane_size(width, height, plane))?;
                }
                Some(total)
            }
        }
    }

    /// Byte offset of the start of the given plane within a frame buffer.
    #[must_use]
    pub fn plane_offset(self, width: u32, height: u32, plane: usize) -> usize {
        let mut offset = 0usize;
        for p in 0..plane {
            offset += self.plane_size(width, height, p);
        }
        offset
    }

    /// Human-readable name matching common ffmpeg-style spelling.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            PixelFormat::Yuv420p => "yuv420p",
            PixelFormat::Yuv422p => "yuv422p",
            PixelFormat::Yuv444p => "yuv444p",
            PixelFormat::Rgb24 => "rgb24",
            PixelFormat::Bgr24 => "bgr24",
            PixelFormat::Rgba8 => "rgba8",
        }
    }
}

impl std::fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Validates that `data` is the right size for a `width x height` frame.
pub fn validate_frame_data(
    format: PixelFormat,
    width: u32,
    height: u32,
    len: usize,
) -> Result<()> {
    let expected = format
        .expected_data_len(width, height)
        .ok_or_else(|| VisualError::InvalidFrame("frame dimensions overflow".into()))?;
    if len != expected {
        return Err(VisualError::InvalidFrame(format!(
            "expected {} bytes for {}x{} {}, got {}",
            expected,
            width,
            height,
            format,
            len
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuv420_layout() {
        assert_eq!(PixelFormat::Yuv420p.num_planes(), 3);
        assert_eq!(PixelFormat::Yuv420p.plane_dimensions(1920, 1080, 1), (960, 540));
        assert_eq!(PixelFormat::Yuv420p.plane_size(1920, 1080, 0), 1920 * 1080);
        assert_eq!(
            PixelFormat::Yuv420p.expected_data_len(2, 2),
            Some(6) // 4 Y + 1 U + 1 V
        );
        // Odd dimensions round chroma up.
        assert_eq!(PixelFormat::Yuv420p.plane_dimensions(3, 3, 1), (2, 2));
    }

    #[test]
    fn yuv422_and_444_layout() {
        assert_eq!(PixelFormat::Yuv422p.plane_dimensions(4, 4, 1), (2, 4));
        assert_eq!(
            PixelFormat::Yuv422p.expected_data_len(4, 4),
            Some(16 + 8 + 8)
        );
        assert_eq!(
            PixelFormat::Yuv444p.expected_data_len(4, 4),
            Some(16 + 16 + 16)
        );
    }

    #[test]
    fn packed_layout() {
        assert_eq!(PixelFormat::Rgb24.expected_data_len(2, 2), Some(12));
        assert_eq!(PixelFormat::Rgba8.expected_data_len(2, 2), Some(16));
        assert_eq!(PixelFormat::Rgba8.bytes_per_pixel(), Some(4));
        assert!(PixelFormat::Yuv420p.bytes_per_pixel().is_none());
    }

    #[test]
    fn plane_offsets() {
        // YUV420: Y plane then U then V.
        assert_eq!(PixelFormat::Yuv420p.plane_offset(4, 4, 0), 0);
        assert_eq!(PixelFormat::Yuv420p.plane_offset(4, 4, 1), 16);
        assert_eq!(PixelFormat::Yuv420p.plane_offset(4, 4, 2), 20);
    }

    #[test]
    fn validation_rejects_bad_size() {
        assert!(validate_frame_data(PixelFormat::Rgba8, 2, 2, 16).is_ok());
        let err = validate_frame_data(PixelFormat::Rgba8, 2, 2, 15).unwrap_err();
        assert!(err.to_string().contains("expected 16 bytes"));
    }

    #[test]
    fn format_flags() {
        assert!(PixelFormat::Yuv420p.is_yuv());
        assert!(!PixelFormat::Rgb24.is_yuv());
        assert!(PixelFormat::Rgba8.has_alpha());
        assert!(!PixelFormat::Yuv444p.has_alpha());
    }
}
