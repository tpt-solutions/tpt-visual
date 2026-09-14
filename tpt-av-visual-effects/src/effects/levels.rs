//! Levels adjustment: input black/white, gamma, output black/white.

use super::param;
use crate::effect::{Effect, EffectPassDesc, EffectParams};
use std::collections::BTreeMap;

const COLOR_CORRECT_WGSL: &str = include_str!("../gpu_shaders/color_correct.wgsl");

/// Per-channel levels.
#[derive(Debug, Clone, PartialEq)]
pub struct Levels {
    /// Input black point (0..1).
    pub in_black: f32,
    /// Input white point (0..1).
    pub in_white: f32,
    /// Midtone gamma (1.0 = neutral).
    pub gamma: f32,
    /// Output black point (0..1).
    pub out_black: f32,
    /// Output white point (0..1).
    pub out_white: f32,
}

impl Levels {
    /// Neutral levels.
    #[must_use]
    pub const fn neutral() -> Self {
        Levels {
            in_black: 0.0,
            in_white: 1.0,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 1.0,
        }
    }

    /// Applies the levels math to one normalized value (CPU reference).
    #[must_use]
    pub fn apply_value(&self, x: f32) -> f32 {
        let span = (self.in_white - self.in_black).max(1e-5);
        let v = ((x - self.in_black) / span).clamp(0.0, 1.0);
        let v = v.powf(1.0 / self.gamma);
        self.out_black + v * (self.out_white - self.out_black)
    }
}

impl Effect for Levels {
    fn name(&self) -> &'static str {
        "levels"
    }

    fn apply_cpu(&self, rgba: &mut [u8], _width: u32, _height: u32) {
        for px in rgba.chunks_exact_mut(4) {
            for slot in px.iter_mut().take(3) {
                let v = f32::from(*slot) / 255.0;
                *slot = (self.apply_value(v).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: COLOR_CORRECT_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        // color_correct.wgsl levels layout: p1 = (in_black, in_white,
        // out_black, out_white), gamma rides in `seed`.
        desc.params.p1 = [self.in_black, self.in_white, self.out_black, self.out_white];
        desc.params.seed = self.gamma;
        desc.params.mode = 1;
        vec![desc]
    }
}

/// Registry constructor.
pub(crate) fn from_params(params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    Box::new(Levels {
        in_black: param(params, "in_black", 0.0),
        in_white: param(params, "in_white", 1.0),
        gamma: param(params, "gamma", 1.0),
        out_black: param(params, "out_black", 0.0),
        out_white: param(params, "out_white", 1.0),
    })
}
