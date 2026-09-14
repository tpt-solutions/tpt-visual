//! Minimal MJPEG AVI writer (no copyleft dependencies).
//!
//! Writes an uncompressed-index AVI 1.0 file with a single `MJPG` video
//! stream. JPEG frames are encoded by the `image` crate.

use std::io::Write;

/// An in-progress MJPEG AVI file.
pub struct AviWriter<W: Write> {
    writer: W,
    width: u32,
    height: u32,
    fps: u32,
    frame_offsets: Vec<u32>,
    frame_sizes: Vec<u32>,
    movi_data: Vec<u8>,
}

impl<W: Write> AviWriter<W> {
    /// Starts an AVI with the given geometry.
    pub fn new(writer: W, width: u32, height: u32, fps: u32) -> Self {
        AviWriter {
            writer,
            width,
            height,
            fps,
            frame_offsets: Vec::new(),
            frame_sizes: Vec::new(),
            movi_data: Vec::new(),
        }
    }

    /// Appends one JPEG-encoded frame.
    pub fn add_jpeg(&mut self, jpeg: &[u8]) -> std::io::Result<()> {
        let offset = self.movi_data.len() as u32 + 4;
        self.frame_offsets.push(offset);
        self.frame_sizes.push(jpeg.len() as u32);
        self.movi_data.extend_from_slice(b"00dc");
        self.movi_data
            .extend_from_slice(&(jpeg.len() as u32).to_le_bytes());
        self.movi_data.extend_from_slice(jpeg);
        if jpeg.len() % 2 == 1 {
            self.movi_data.push(0); // word alignment
        }
        Ok(())
    }

    /// Encodes an RGBA buffer as JPEG and appends it.
    pub fn add_rgba(&mut self, rgba: &[u8], width: u32, height: u32, quality: u8) -> std::io::Result<()> {
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for px in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
        }
        let mut jpeg = Vec::new();
        {
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
            encoder
                .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        }
        self.add_jpeg(&jpeg)
    }

    fn chunk(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 8);
        out.extend_from_slice(name);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn list(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 12);
        out.extend_from_slice(b"LIST");
        out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(body);
        if (body.len() + 4) % 2 == 1 {
            out.push(0);
        }
        out
    }

    /// Finalizes and writes the file.
    pub fn finish(mut self) -> std::io::Result<u64> {
        let frames = self.frame_offsets.len() as u32;
        let (w, h) = (self.width, self.height);
        let fps = self.fps;
        let frame_len_us = 1_000_000_u32 / fps.max(1);

        // avih: main header (56 bytes body).
        let mut avih = Vec::new();
        avih.extend_from_slice(&frame_len_us.to_le_bytes()); // usec per frame
        avih.extend_from_slice((frames.wrapping_mul(frame_len_us)).to_le_bytes().as_slice());
        avih.extend_from_slice(&0_u32.to_le_bytes()); // max bytes per sec
        avih.extend_from_slice(&0_u32.to_le_bytes()); // padding
        avih.extend_from_slice(&0x10_u32.to_le_bytes()); // flags: has index
        avih.extend_from_slice(&frames.to_le_bytes()); // total frames
        avih.extend_from_slice(&0_u32.to_le_bytes()); // initial streams
        avih.extend_from_slice(&1_u32.to_le_bytes()); // stream count
        avih.extend_from_slice(&(w * h * 3).to_le_bytes()); // suggested buffer
        avih.extend_from_slice(&w.to_le_bytes());
        avih.extend_from_slice(&h.to_le_bytes());
        avih.extend_from_slice(&[0_u8; 16]); // reserved
        let avih = Self::chunk(b"avih", &avih);

        // strl: stream header + format.
        let mut strh = Vec::new();
        strh.extend_from_slice(b"vids");
        strh.extend_from_slice(b"MJPG");
        strh.extend_from_slice(&0_u32.to_le_bytes()); // flags
        strh.extend_from_slice(&0_u16.to_le_bytes()); // priority
        strh.extend_from_slice(&0_u16.to_le_bytes()); // language
        strh.extend_from_slice(&0_u32.to_le_bytes()); // initial frames
        strh.extend_from_slice(&1_u32.to_le_bytes()); // scale
        strh.extend_from_slice(&fps.to_le_bytes()); // rate
        strh.extend_from_slice(&0_u32.to_le_bytes()); // start
        strh.extend_from_slice(&frames.to_le_bytes()); // length
        strh.extend_from_slice(&(w * h * 3).to_le_bytes()); // suggested buffer
        strh.extend_from_slice(&0xffff_ffff_u32.to_le_bytes()); // quality (-1)
        strh.extend_from_slice(&0_u32.to_le_bytes()); // sample size
        strh.extend_from_slice(&[0_u8; 8]); // frame rect
        let strh = Self::chunk(b"strh", &strh);

        let mut strf = Vec::new();
        strf.extend_from_slice(&40_u32.to_le_bytes()); // BITMAPINFOHEADER size
        strf.extend_from_slice(&w.to_le_bytes());
        strf.extend_from_slice(&h.to_le_bytes());
        strf.extend_from_slice(&1_u16.to_le_bytes()); // planes
        strf.extend_from_slice(&24_u16.to_le_bytes()); // bit count
        strf.extend_from_slice(b"MJPG");
        strf.extend_from_slice(&(w * h * 3).to_le_bytes()); // image size
        strf.extend_from_slice(&0_u32.to_le_bytes()); // x pixels per meter
        strf.extend_from_slice(&0_u32.to_le_bytes()); // y pixels per meter
        strf.extend_from_slice(&0_u32.to_le_bytes()); // colors used
        strf.extend_from_slice(&0_u32.to_le_bytes()); // important colors
        let strf = Self::chunk(b"strf", &strf);
        let strl = Self::list(b"strl", &[strh, strf].concat());
        let hdrl = Self::list(b"hdrl", &[avih, strl].concat());

        let movi = Self::list(b"movi", &self.movi_data);

        // idx1 index.
        let mut idx1 = Vec::with_capacity(self.frame_offsets.len() * 16);
        for (offset, size) in self
            .frame_offsets
            .iter()
            .zip(&self.frame_sizes)
        {
            idx1.extend_from_slice(b"00dc");
            idx1.extend_from_slice(&0x10_u32.to_le_bytes()); // AVIIF_KEYFRAME
            idx1.extend_from_slice(offset.to_le_bytes().as_slice());
            idx1.extend_from_slice(size.to_le_bytes().as_slice());
        }
        let idx1 = Self::chunk(b"idx1", &idx1);

        let body_len = hdrl.len() + movi.len() + idx1.len();
        let mut riff = Vec::with_capacity(body_len + 8);
        riff.extend_from_slice(b"RIFF");
        riff.extend_from_slice(&((body_len + 4) as u32).to_le_bytes());
        riff.extend_from_slice(b"AVI ");
        riff.extend_from_slice(&hdrl);
        riff.extend_from_slice(&movi);
        riff.extend_from_slice(&idx1);

        self.writer.write_all(&riff)?;
        Ok(riff.len() as u64)
    }
}
