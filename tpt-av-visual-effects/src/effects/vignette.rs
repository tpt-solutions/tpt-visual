//! Vignette: radial darkening toward the frame edges.

use super::{param, smoothstep};
use crate::effect::{Effect, EffectParams, EffectPassDesc};
use std::collections::BTreeMap;

const VIGNETTE_WGSL: &str = include_str!("../gpu_shaders/vignette.wgsl");

/// A vignette effect. `amount` 0 = off; `radius` and `softness` are
/// normalized (1.0 = half-diagonal distance from center).
#[derive(Debug, Clone, PartialEq)]
pub struct Vignette {
    /// Darkening strength at the corners (0..1).
    pub amount: f32,
    /// Radius where the falloff is centered.
    pub radius: f32,
    /// Falloff softness.
    pub softness: f32,
}

impl Vignette {
    /// A gentle default vignette.
    #[must_use]
    pub fn gentle() -> Self {
        Vignette {
            amount: 0.5,
            radius: 0.75,
            softness: 0.5,
        }
    }

    /// The multiplicative brightness factor at normalized distance `d`
    /// from the center (CPU reference matching the shader).
    #[must_use]
    pub fn factor_at(&self, d: f32) -> f32 {
        let x = d / self.radius.max(1e-4);
        1.0 - self.amount * smoothstep(1.0 - self.softness, 1.0 + self.softness, x)
    }
}

impl Effect for Vignette {
    fn name(&self) -> &'static str {
        "vignette"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        // Matches the shader: UV-space distance from the frame center
        // (aspect-dependent, the standard cheap vignette).
        for y in 0..height {
            for x in 0..width {
                let ux = x as f32 / width as f32;
                let uy = y as f32 / height as f32;
                let d = (ux - 0.5).hypot(uy - 0.5);
                let factor = self.factor_at(d);
                let idx = ((y * width + x) * 4) as usize;
                for slot in rgba[idx..idx + 3].iter_mut() {
                    *slot = (f32::from(*slot) * factor).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: VIGNETTE_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [self.amount, self.radius, self.softness, 0.0];
        vec![desc]
    }
}

/// Registry constructor.
pub(crate) fn from_params(params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    Box::new(Vignette {
        amount: param(params, "amount", 0.5),
        radius: param(params, "radius", 0.75),
        softness: param(params, "softness", 0.5),
    })
}
