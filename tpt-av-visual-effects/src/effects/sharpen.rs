//! Sharpening / edge enhancement (unsharp mask).

use crate::effect::{Effect, EffectParams, EffectPassDesc};

const SHARPEN_WGSL: &str = include_str!("../gpu_shaders/sharpen.wgsl");

/// Unsharp mask over a 3x3 neighborhood. `amount` 0 = no change; 1..3 are
/// typical values.
#[derive(Debug, Clone, PartialEq)]
pub struct Sharpen {
    /// Edge enhancement strength.
    pub amount: f32,
}

impl Sharpen {
    /// Creates a sharpen effect with the given amount.
    #[must_use]
    pub fn new(amount: f32) -> Self {
        Sharpen { amount }
    }
}

impl Effect for Sharpen {
    fn name(&self) -> &'static str {
        "sharpen"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        let src = rgba.to_vec();
        for y in 0..height {
            for x in 0..width {
                let c = super::sample_clamped(&src, width, height, x.into(), y.into());
                let mut blur = [0.0_f32; 4];
                for dy in -1_i64..=1 {
                    for dx in -1_i64..=1 {
                        let s = super::sample_clamped(
                            &src,
                            width,
                            height,
                            x as i64 + dx,
                            y as i64 + dy,
                        );
                        for ch in 0..3 {
                            blur[ch] += s[ch] / 9.0;
                        }
                    }
                }
                let mut out = [c[0], c[1], c[2], c[3]];
                for ch in 0..3 {
                    out[ch] = c[ch] + (c[ch] - blur[ch]) * self.amount;
                }
                super::write_pixel(rgba, width, x, y, out);
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: SHARPEN_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [self.amount, 0.0, 0.0, 0.0];
        vec![desc]
    }
}
