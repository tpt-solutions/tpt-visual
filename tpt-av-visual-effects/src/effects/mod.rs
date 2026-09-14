//! Built-in effects.

pub mod blur;
pub mod chroma_key;
pub mod color_correct;
pub mod curves;
pub mod levels;
pub mod noise;
pub mod sharpen;
pub mod vignette;

pub use blur::{BoxBlur, GaussianBlur, MotionBlur};
pub use chroma_key::ChromaKey;
pub use color_correct::ColorCorrect;
pub use curves::{Curve, ToneCurve};
pub use levels::Levels;
pub use noise::{Noise, NoiseReduction};
pub use sharpen::Sharpen;
pub use vignette::Vignette;

/// Reads a parameter from a numeric bag with a fallback default.
#[must_use]
pub fn param(params: &std::collections::BTreeMap<String, f32>, key: &str, default: f32) -> f32 {
    params.get(key).copied().unwrap_or(default)
}

/// Clamped pixel fetch for CPU kernels (edge-replicate).
#[inline]
#[must_use]
pub fn sample_clamped(rgba: &[u8], width: u32, height: u32, x: i64, y: i64) -> [f32; 4] {
    let x = x.clamp(0, i64::from(width) - 1) as u32;
    let y = y.clamp(0, i64::from(height) - 1) as u32;
    let idx = ((y * width + x) * 4) as usize;
    [
        f32::from(rgba[idx]) / 255.0,
        f32::from(rgba[idx + 1]) / 255.0,
        f32::from(rgba[idx + 2]) / 255.0,
        f32::from(rgba[idx + 3]) / 255.0,
    ]
}

/// Writes a normalized pixel back into the buffer.
#[inline]
pub fn write_pixel(rgba: &mut [u8], width: u32, x: u32, y: u32, px: [f32; 4]) {
    let idx = ((y * width + x) * 4) as usize;
    for (slot, v) in rgba[idx..idx + 4].iter_mut().zip(px) {
        *slot = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

