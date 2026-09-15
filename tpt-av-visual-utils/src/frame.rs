//! Video frame types.
//!
//! [`VideoFrame`] is the CPU-side frame representation used across the whole
//! visual stack. Decoders (e.g. `tpt-kinetix`) convert their native frames
//! into this type at the boundary.

use serde::{Deserialize, Serialize};

use crate::pixel_format::{validate_frame_data, PixelFormat};
use crate::{Result, VisualError};

/// A decoded video frame (CPU-side, ready for GPU upload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Pixel / chroma-sampling format.
    pub pixel_format: PixelFormat,
    /// Raw plane data. Layout depends on `pixel_format`; planar YUV formats
    /// store their planes contiguously in Y, U, V order.
    pub data: Vec<u8>,
    /// Presentation frame index relative to the start of the source asset.
    pub frame_number: u64,
    /// Whether this frame is a random-access (key) frame.
    pub is_key_frame: bool,
}

impl VideoFrame {
    /// Creates a new frame, validating that `data` matches the declared
    /// format and dimensions.
    pub fn new(
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
        data: Vec<u8>,
        frame_number: u64,
    ) -> Result<Self> {
        validate_frame_data(pixel_format, width, height, data.len())?;
        Ok(Self {
            width,
            height,
            pixel_format,
            data,
            frame_number,
            is_key_frame: false,
        })
    }

    /// Convenience constructor for fully opaque RGBA frames.
    pub fn rgba(width: u32, height: u32, frame_number: u64) -> Self {
        let len = PixelFormat::Rgba8
            .expected_data_len(width, height)
            .expect("dimensions do not overflow");
        Self {
            width,
            height,
            pixel_format: PixelFormat::Rgba8,
            data: vec![0; len],
            frame_number,
            is_key_frame: true,
        }
    }

    /// Returns the byte slice for the given plane (0-based).
    pub fn plane(&self, plane: usize) -> Result<&[u8]> {
        if plane >= self.pixel_format.num_planes() {
            return Err(VisualError::InvalidFrame(format!(
                "plane {} out of range for {}",
                plane, self.pixel_format
            )));
        }
        let offset = self
            .pixel_format
            .plane_offset(self.width, self.height, plane);
        let size = self.pixel_format.plane_size(self.width, self.height, plane);
        self.data
            .get(offset..offset + size)
            .ok_or_else(|| VisualError::invalid_frame("plane data truncated"))
    }

    /// Mutable variant of [`VideoFrame::plane`].
    pub fn plane_mut(&mut self, plane: usize) -> Result<&mut [u8]> {
        let offset = self
            .pixel_format
            .plane_offset(self.width, self.height, plane);
        let size = self.pixel_format.plane_size(self.width, self.height, plane);
        self.data
            .get_mut(offset..offset + size)
            .ok_or_else(|| VisualError::invalid_frame("plane data truncated"))
    }

    /// Converts this frame to a packed RGBA8 buffer (one row at a time,
    /// top row first).
    ///
    /// This is the CPU reference conversion. The compositor performs YUV →
    /// RGB on the GPU; this path exists for tests, thumbnails, and
    /// software fallbacks. YUV frames are interpreted as ITU-R BT.709,
    /// limited range (the broadcast default).
    #[must_use]
    pub fn to_rgba(&self) -> Vec<u8> {
        match self.pixel_format {
            PixelFormat::Rgba8 => self.data.clone(),
            PixelFormat::Rgb24 => packed_to_rgba(&self.data, 3),
            PixelFormat::Bgr24 => packed_bgr_to_rgba(&self.data),
            PixelFormat::Yuv420p => self.yuv_to_rgba(1, 1),
            PixelFormat::Yuv422p => self.yuv_to_rgba(1, 0),
            PixelFormat::Yuv444p => self.yuv_to_rgba(0, 0),
        }
    }

    /// Creates an RGBA8 frame from a packed RGBA buffer.
    #[must_use]
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>, frame_number: u64) -> Self {
        debug_assert_eq!(
            Some(rgba.len()),
            PixelFormat::Rgba8.expected_data_len(width, height)
        );
        Self {
            width,
            height,
            pixel_format: PixelFormat::Rgba8,
            data: rgba,
            frame_number,
            is_key_frame: true,
        }
    }

    /// YUV (BT.709, limited range) → RGBA. `hs`/`vs` are the chroma
    /// horizontal/vertical subsampling shifts (0 = 4:4:4).
    fn yuv_to_rgba(&self, hs: u32, vs: u32) -> Vec<u8> {
        let w = self.width as usize;
        let h = self.height as usize;
        let cw = (w + (1 << hs) - 1) >> hs;
        let ch = (h + (1 << vs) - 1) >> vs;
        let y_plane = &self.data[0..w * h];
        let u_plane = &self.data[w * h..w * h + cw * ch];
        let v_plane = &self.data[w * h + cw * ch..w * h + 2 * cw * ch];

        let mut out = vec![0u8; w * h * 4];
        for row in 0..h {
            for col in 0..w {
                let y = f32::from(y_plane[row * w + col]);
                let u = f32::from(u_plane[(row >> vs) * cw + (col >> hs)]) - 128.0;
                let v = f32::from(v_plane[(row >> vs) * cw + (col >> hs)]) - 128.0;
                // BT.709 limited-range expansion.
                let y_full = (y - 16.0) * (255.0 / 219.0);
                let r = y_full + 1.5748 * v;
                let g = y_full - 0.1873 * u - 0.4681 * v;
                let b = y_full + 1.8556 * u;
                let idx = (row * w + col) * 4;
                out[idx] = clamp_u8(r);
                out[idx + 1] = clamp_u8(g);
                out[idx + 2] = clamp_u8(b);
                out[idx + 3] = 255;
            }
        }
        out
    }
}

fn clamp_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn packed_to_rgba(data: &[u8], bpp: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / bpp * 4);
    for px in data.chunks_exact(bpp) {
        out.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    out
}

fn packed_bgr_to_rgba(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 3 * 4);
    for px in data.chunks_exact(3) {
        out.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_validates_data_len() {
        let frame = VideoFrame::new(2, 2, PixelFormat::Rgba8, vec![0; 16], 0);
        assert!(frame.is_ok());
        let err = VideoFrame::new(2, 2, PixelFormat::Rgba8, vec![0; 15], 0).unwrap_err();
        assert!(matches!(err, VisualError::InvalidFrame(_)));
    }

    #[test]
    fn plane_slices() {
        let mut frame = VideoFrame::new(4, 4, PixelFormat::Yuv420p, vec![0; 24], 0).unwrap();
        assert_eq!(frame.plane(0).unwrap().len(), 16);
        assert_eq!(frame.plane(1).unwrap().len(), 4);
        assert_eq!(frame.plane(2).unwrap().len(), 4);
        assert!(frame.plane(3).is_err());
        frame.plane_mut(0).unwrap().fill(235);
        assert_eq!(frame.plane(0).unwrap()[0], 235);
    }

    #[test]
    fn yuv_grey_roundtrips_to_grey() {
        // BT.709 limited range: luma 235 (plus neutral chroma 128) == white.
        let w = 2;
        let h = 2;
        let mut data = vec![235u8; w * h];
        data.extend_from_slice(&[128u8; 1]); // U
        data.extend_from_slice(&[128u8; 1]); // V
        let frame = VideoFrame::new(2, 2, PixelFormat::Yuv420p, data, 0).unwrap();
        let rgba = frame.to_rgba();
        for px in rgba.chunks_exact(4) {
            assert_eq!([px[0], px[1], px[2]], [255, 255, 255]);
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn yuv_black_is_level_16() {
        let w = 2;
        let h = 2;
        let mut data = vec![16u8; w * h];
        data.extend_from_slice(&[128u8; 1]); // U
        data.extend_from_slice(&[128u8; 1]); // V
        let frame = VideoFrame::new(2, 2, PixelFormat::Yuv420p, data, 0).unwrap();
        let rgba = frame.to_rgba();
        for px in rgba.chunks_exact(4) {
            assert_eq!([px[0], px[1], px[2]], [0, 0, 0]);
        }
    }

    #[test]
    fn packed_conversions() {
        let frame = VideoFrame::new(1, 2, PixelFormat::Rgb24, vec![1, 2, 3, 4, 5, 6], 0).unwrap();
        assert_eq!(frame.to_rgba(), vec![1, 2, 3, 255, 4, 5, 6, 255]);

        let frame = VideoFrame::new(1, 1, PixelFormat::Bgr24, vec![1, 2, 3], 0).unwrap();
        assert_eq!(frame.to_rgba(), vec![3, 2, 1, 255]);
    }

    #[test]
    fn rgba_passthrough() {
        let frame = VideoFrame::from_rgba(1, 1, vec![9, 8, 7, 6], 7);
        assert_eq!(frame.to_rgba(), vec![9, 8, 7, 6]);
        assert_eq!(frame.frame_number, 7);
    }
}
