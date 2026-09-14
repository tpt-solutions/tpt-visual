//! Frame decoder abstraction plus built-in implementations.
//!
//! [`FrameDecoder`] decodes indexed frames of one asset. Implementations:
//! - [`ProceduralDecoder`] — animated test pattern (no media files needed).
//! - [`ImageSequenceDecoder`] — numbered PNG/JPEG frames on disk.
//! - [`KinetixDecoder`] — real MP4/H.264 media via `tpt-kinetix` (behind the
//!   `kinetix` feature).

use crate::gpu::device::Result;
use std::path::PathBuf;
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution, VideoFrame};

/// Decodes indexed frames of a single asset.
pub trait FrameDecoder: Send {
    /// Decodes the frame at `index` (blocking). Repeated or out-of-order
    /// requests may re-open/seek internally.
    fn decode_frame(&mut self, index: u64) -> Result<VideoFrame>;

    /// Nominal frame rate of the decoded stream.
    fn frame_rate(&self) -> FrameRate;

    /// Nominal resolution of the decoded stream.
    fn resolution(&self) -> Resolution;
}

/// An animated procedural test pattern (moving diagonal stripes). Used by
/// tests, examples, and fallback rendering.
pub struct ProceduralDecoder {
    frame_rate: FrameRate,
    resolution: Resolution,
}

impl ProceduralDecoder {
    /// A procedural source at the given rate/resolution.
    #[must_use]
    pub fn new(frame_rate: FrameRate, resolution: Resolution) -> Self {
        ProceduralDecoder {
            frame_rate,
            resolution,
        }
    }

    /// Renders the pattern for frame `index` into an RGBA buffer.
    #[must_use]
    pub fn render(&self, index: u64) -> VideoFrame {
        let w = self.resolution.width;
        let h = self.resolution.height;
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        let phase = (index % 60) as f32 / 60.0;
        for y in 0..h {
            for x in 0..w {
                let t = ((x + y) as f32 / 24.0 + phase * std::f32::consts::TAU).sin();
                let r = (t * 0.5 + 0.5) * 255.0;
                let g = (x * 255) as f32 / w.max(1) as f32;
                let b = (y * 255) as f32 / h.max(1) as f32;
                data.extend_from_slice(&[r as u8, g as u8, b as u8, 255]);
            }
        }
        VideoFrame::from_rgba(w, h, data, index)
    }
}

impl FrameDecoder for ProceduralDecoder {
    fn decode_frame(&mut self, index: u64) -> Result<VideoFrame> {
        Ok(self.render(index))
    }

    fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}

/// Numbered image files (e.g. `frame_0001.png`) as a video source.
pub struct ImageSequenceDecoder {
    dir: PathBuf,
    prefix: String,
    digits: usize,
    extension: String,
    frame_rate: FrameRate,
    resolution: Resolution,
    cache: std::collections::HashMap<u64, VideoFrame>,
}

impl ImageSequenceDecoder {
    /// Opens `dir/{prefix}{index:0digits$}.{ext}` as a frame sequence.
    pub fn new(
        dir: impl Into<PathBuf>,
        prefix: impl Into<String>,
        digits: usize,
        extension: impl Into<String>,
        frame_rate: FrameRate,
        resolution: Resolution,
    ) -> Self {
        ImageSequenceDecoder {
            dir: dir.into(),
            prefix: prefix.into(),
            digits,
            extension: extension.into(),
            frame_rate,
            resolution,
            cache: std::collections::HashMap::new(),
        }
    }

    fn path_for(&self, index: u64) -> PathBuf {
        self.dir.join(format!(
            "{}{:0width$}.{}",
            self.prefix,
            index,
            self.extension,
            width = self.digits
        ))
    }
}

impl FrameDecoder for ImageSequenceDecoder {
    fn decode_frame(&mut self, index: u64) -> Result<VideoFrame> {
        if let Some(frame) = self.cache.get(&index) {
            return Ok(frame.clone());
        }
        let path = self.path_for(index);
        let img = image::open(&path)
            .map_err(|e| crate::gpu::device::CompositorError::Decode(format!(
                "{}: {e}",
                path.display()
            )))?
            .to_rgba8();
        let (w, h) = img.dimensions();
        Ok(VideoFrame::from_rgba(w, h, img.into_raw(), index))
    }

    fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}

/// Decodes MP4 (H.264) media with `tpt-kinetix`.
#[cfg(feature = "kinetix")]
pub struct KinetixDecoder {
    data: Vec<u8>,
    frame_rate: FrameRate,
    resolution: Resolution,
    demuxer: Option<tpt_kinetix_demux::Mp4Demuxer>,
    decoder: Option<tpt_kinetix_h264::H264Decoder>,
    cursor: u64,
}

#[cfg(feature = "kinetix")]
impl KinetixDecoder {
    /// Loads media bytes (the full file). Decoders are opened lazily so a
    /// cache can be constructed on any thread.
    pub fn from_bytes(
        data: Vec<u8>,
        frame_rate: FrameRate,
        resolution: Resolution,
    ) -> Self {
        KinetixDecoder {
            data,
            frame_rate,
            resolution,
            demuxer: None,
            decoder: None,
            cursor: 0,
        }
    }

    fn open(&mut self) -> Result<()> {
        let demuxer = tpt_kinetix_demux::Mp4Demuxer::new(self.data.clone())
            .map_err(|e| crate::gpu::device::CompositorError::Decode(e.to_string()))?;
        self.demuxer = Some(demuxer);
        self.decoder = Some(tpt_kinetix_h264::H264Decoder::new().with_display_order());
        self.cursor = 0;
        Ok(())
    }
}

#[cfg(feature = "kinetix")]
use tpt_kinetix_demux::Demuxer as _;

#[cfg(feature = "kinetix")]
impl FrameDecoder for KinetixDecoder {
    fn decode_frame(&mut self, index: u64) -> Result<VideoFrame> {
        // Sequential decode in display order; rewind when seeking back.
        if self.demuxer.is_none() || index < self.cursor {
            self.open()?;
        }
        loop {
            let frame_index = self.cursor;
            let pkt = {
                let demuxer = self.demuxer.as_mut().expect("opened above");
                match demuxer.read_packet() {
                    Ok(Some(pkt)) => pkt,
                    Ok(None) => {
                        return Err(crate::gpu::device::CompositorError::Decode(format!(
                            "frame {index} beyond end of stream"
                        )))
                    }
                    Err(e) => {
                        return Err(crate::gpu::device::CompositorError::Decode(e.to_string()))
                    }
                }
            };
            let decoded = {
                let decoder = self.decoder.as_mut().expect("opened above");
                decoder.decode(&pkt)
            };
            self.cursor = frame_index + 1;
            if let Ok(Some(kinetix_frame)) = decoded {
                // Convert kinetix frame data into the visual-stack frame type.
                let (w, h) = (kinetix_frame.width, kinetix_frame.height);
                let pf = match kinetix_frame.pixel_format {
                    tpt_kinetix_core::PixelFormat::Yuv420p => PixelFormat::Yuv420p,
                    tpt_kinetix_core::PixelFormat::Yuv422p => PixelFormat::Yuv422p,
                    tpt_kinetix_core::PixelFormat::Yuv444p => PixelFormat::Yuv444p,
                    tpt_kinetix_core::PixelFormat::Rgb24 => PixelFormat::Rgb24,
                    tpt_kinetix_core::PixelFormat::Bgr24 => PixelFormat::Bgr24,
                };
                return VideoFrame::new(w, h, pf, kinetix_frame.data, frame_index)
                    .map_err(|e| crate::gpu::device::CompositorError::Decode(e.to_string()))
                    .map(|mut f| {
                        f.is_key_frame = kinetix_frame.is_key_frame;
                        f
                    });
            }
        }
    }

    fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}
