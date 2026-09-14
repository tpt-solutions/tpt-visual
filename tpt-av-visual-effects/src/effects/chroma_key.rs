//! Green/blue screen keying with spill suppression.

use super::{param, smoothstep};
use crate::effect::{Effect, EffectPassDesc, EffectParams};
use std::collections::BTreeMap;

const CHROMA_KEY_WGSL: &str = include_str!("../gpu_shaders/chroma_key.wgsl");

/// Chroma keyer: pixels whose chroma is close to `key_color` become
/// transparent, with feathered edges and spill suppression.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromaKey {
    /// The key color in 0..1 RGB (e.g. pure green `[0.0, 1.0, 0.0]`).
    pub key_color: [f32; 3],
    /// Chroma distance below which pixels are fully keyed.
    pub tolerance: f32,
    /// Feather width above the tolerance.
    pub softness: f32,
    /// Spill suppression strength (0 = off, 1 = full).
    pub spill_suppression: f32,
}

impl ChromaKey {
    /// A green screen keyer with typical defaults.
    #[must_use]
    pub fn green_screen() -> Self {
        ChromaKey {
            key_color: [0.0, 1.0, 0.0],
            tolerance: 0.3,
            softness: 0.2,
            spill_suppression: 0.5,
        }
    }

    /// Chroma distance between a pixel and the key (CPU reference matching
    /// the shader's weighted formula).
    #[must_use]
    pub fn chroma_distance(c: [f32; 3], key: [f32; 3]) -> f32 {
        let d = [
            (c[0] - key[0]) * 0.6,
            (c[1] - key[1]) * 0.3,
            (c[2] - key[2]) * 0.6,
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

impl Effect for ChromaKey {
    fn name(&self) -> &'static str {
        "chroma_key"
    }

    fn apply_cpu(&self, rgba: &mut [u8], _width: u32, _height: u32) {
        for px in rgba.chunks_exact_mut(4) {
            let c = [
                f32::from(px[0]) / 255.0,
                f32::from(px[1]) / 255.0,
                f32::from(px[2]) / 255.0,
            ];
            let d = Self::chroma_distance(c, self.key_color);
            let alpha =
                smoothstep(self.tolerance, self.tolerance + self.softness, d);

            let mut rgb = c;
            if self.spill_suppression > 0.0 {
                let spill = self.spill_suppression * (1.0 - alpha);
                if self.key_color[1] >= self.key_color[0].max(self.key_color[2]) {
                    let neutral = (rgb[0] + rgb[2]) * 0.5;
                    rgb[1] = rgb[1] + (rgb[1].min(neutral) - rgb[1]) * spill;
                } else if self.key_color[2] > self.key_color[0] {
                    let neutral = (rgb[0] + rgb[1]) * 0.5;
                    rgb[2] = rgb[2] + (rgb[2].min(neutral) - rgb[2]) * spill;
                } else {
                    let neutral = (rgb[1] + rgb[2]) * 0.5;
                    rgb[0] = rgb[0] + (rgb[0].min(neutral) - rgb[0]) * spill;
                }
            }

            px[0] = (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            px[1] = (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            px[2] = (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8;
            px[3] = ((f32::from(px[3]) / 255.0) * alpha * 255.0).round() as u8;
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: CHROMA_KEY_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [self.key_color[0], self.key_color[1], self.key_color[2], self.tolerance];
        desc.params.p1 = [self.softness, self.spill_suppression, 0.0, 0.0];
        vec![desc]
    }
}

/// Registry constructor: `key_r/key_g/key_b/tolerance/softness/spill`.
pub(crate) fn from_params(params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    Box::new(ChromaKey {
        key_color: [
            param(params, "key_r", 0.0),
            param(params, "key_g", 1.0),
            param(params, "key_b", 0.0),
        ],
        tolerance: param(params, "tolerance", 0.3),
        softness: param(params, "softness", 0.2),
        spill_suppression: param(params, "spill", 0.5),
    })
}
