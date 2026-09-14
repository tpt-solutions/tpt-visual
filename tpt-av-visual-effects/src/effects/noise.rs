//! Noise generation (film grain) and reduction.

use super::param;
use crate::effect::{Effect, EffectPassDesc, EffectParams};
use std::collections::BTreeMap;

const NOISE_WGSL: &str = include_str!("../gpu_shaders/noise.wgsl");

/// Deterministic hash matching the shader's `hash()` so CPU and GPU grain
/// line up closely (exact float parity is not guaranteed across GPUs, but
/// the statistical properties match).
fn grain_hash(x: f32, y: f32, seed: f32) -> f32 {
    let h = x * 127.1 + y * 311.7 + seed * 74.7;
    let s = h.sin() * 43_758.5453;
    s - s.floor()
}

/// Film-grain style noise generation.
#[derive(Debug, Clone, PartialEq)]
pub struct Noise {
    /// Noise amplitude (0..1 of full scale).
    pub amount: f32,
    /// Monochrome grain when true (luma-only noise).
    pub monochrome: bool,
    /// Deterministic seed.
    pub seed: f32,
}

impl Noise {
    /// Creates a grain generator.
    #[must_use]
    pub fn new(amount: f32, seed: f32) -> Self {
        Noise {
            amount,
            monochrome: true,
            seed,
        }
    }
}

impl Effect for Noise {
    fn name(&self) -> &'static str {
        "noise"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        for y in 0..height {
            for x in 0..width {
                let n0 = grain_hash(x as f32, y as f32, self.seed);
                let n1 = grain_hash(x as f32 + 17.0, y as f32 + 43.0, self.seed);
                let n2 = grain_hash(x as f32 + 91.0, y as f32 + 7.0, self.seed);
                let idx = ((y * width + x) * 4) as usize;
                let channels = if self.monochrome {
                    [n0, n0, n0]
                } else {
                    [n0, n1, n2]
                };
                for ch in 0..3 {
                    let v = f32::from(rgba[idx + ch]) / 255.0;
                    let out = v + (channels[ch] - 0.5) * self.amount;
                    rgba[idx + ch] = (out.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: NOISE_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [self.amount, u32::from(self.monochrome) as f32, 0.0, 0.0];
        desc.params.mode = 0;
        desc.params.seed = self.seed;
        vec![desc]
    }
}

/// Noise reduction: bilateral-weighted 3x3 filter.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseReduction {
    /// Blend strength toward the filtered result (0..1).
    pub strength: f32,
}

impl NoiseReduction {
    /// Creates a noise reducer.
    #[must_use]
    pub fn new(strength: f32) -> Self {
        NoiseReduction {
            strength: strength.clamp(0.0, 1.0),
        }
    }
}

impl Effect for NoiseReduction {
    fn name(&self) -> &'static str {
        "noise_reduction"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        let src = rgba.to_vec();
        for y in 0..height {
            for x in 0..width {
                let center = super::sample_clamped(&src, width, height, x.into(), y.into());
                let mut sum = [0.0_f32; 3];
                let mut weight_sum = 0.0;
                for dy in -1_i64..=1 {
                    for dx in -1_i64..=1 {
                        let s = super::sample_clamped(&src, width, height, x as i64 + dx, y as i64 + dy);
                        let dist2 = {
                            let d = [
                                s[0] - center[0],
                                s[1] - center[1],
                                s[2] - center[2],
                            ];
                            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
                        };
                        let w = (-dist2 * 32.0).exp();
                        for ch in 0..3 {
                            sum[ch] += s[ch] * w;
                        }
                        weight_sum += w;
                    }
                }
                let mut out = [center[0], center[1], center[2], center[3]];
                for ch in 0..3 {
                    let filtered = sum[ch] / weight_sum;
                    out[ch] = center[ch] + (filtered - center[ch]) * self.strength;
                }
                super::write_pixel(rgba, width, x, y, out);
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: NOISE_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [self.strength, 0.0, 0.0, 0.0];
        desc.params.mode = 1;
        vec![desc]
    }
}

/// Registry constructors.
pub(crate) fn from_params(name: &'static str, params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    match name {
        "noise" => Box::new(Noise {
            amount: param(params, "amount", 0.1),
            monochrome: param(params, "monochrome", 1.0) > 0.5,
            seed: param(params, "seed", 0.0),
        }),
        "noise_reduction" => Box::new(NoiseReduction::new(param(params, "strength", 0.5))),
        _ => unreachable!("registry dispatch checked upstream"),
    }
}
