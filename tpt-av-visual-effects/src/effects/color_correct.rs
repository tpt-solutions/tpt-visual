//! Primary color correction: brightness, contrast, saturation, hue.

use super::param;
use crate::effect::{Effect, EffectPassDesc, EffectParams};
use std::collections::BTreeMap;

const COLOR_CORRECT_WGSL: &str = include_str!("../gpu_shaders/color_correct.wgsl");

/// Brightness / contrast / saturation / hue adjustment.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorCorrect {
    /// Additive brightness offset (-1..1).
    pub brightness: f32,
    /// Contrast around 0.5 pivot (-1..1, 0 = neutral).
    pub contrast: f32,
    /// Saturation multiplier (1.0 = neutral, 0 = greyscale).
    pub saturation: f32,
    /// Hue rotation in degrees.
    pub hue_degrees: f32,
}

impl ColorCorrect {
    /// A neutral correction.
    #[must_use]
    pub const fn neutral() -> Self {
        ColorCorrect {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 1.0,
            hue_degrees: 0.0,
        }
    }
}

impl Effect for ColorCorrect {
    fn name(&self) -> &'static str {
        "color_correct"
    }

    fn apply_cpu(&self, rgba: &mut [u8], _width: u32, _height: u32) {
        for px in rgba.chunks_exact_mut(4) {
            let mut c = [
                f32::from(px[0]) / 255.0 + self.brightness,
                f32::from(px[1]) / 255.0 + self.brightness,
                f32::from(px[2]) / 255.0 + self.brightness,
            ];
            for v in &mut c {
                *v = (*v - 0.5) * (1.0 + self.contrast) + 0.5;
            }
            let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            for v in &mut c {
                *v = luma + (*v - luma) * self.saturation;
            }
            if self.hue_degrees.abs() > 1e-3 {
                c = rotate_hue(c, self.hue_degrees);
            }
            for (slot, v) in px.iter_mut().take(3).zip(c) {
                *slot = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut desc = EffectPassDesc {
            shader_source: COLOR_CORRECT_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [
            self.brightness,
            self.contrast,
            self.saturation,
            self.hue_degrees,
        ];
        vec![desc]
    }
}

/// HSV hue rotation (CPU reference matching the shader's HSV path).
#[must_use]
pub fn rotate_hue(c: [f32; 3], degrees: f32) -> [f32; 3] {
    let v = c[0].max(c[1]).max(c[2]);
    let min_c = c[0].min(c[1]).min(c[2]);
    let delta = v - min_c;
    let mut h = if delta > 0.0 {
        let raw = if v == c[0] {
            (c[1] - c[2]) / delta
        } else if v == c[1] {
            2.0 + (c[2] - c[0]) / delta
        } else {
            4.0 + (c[0] - c[1]) / delta
        } / 6.0;
        if raw < 0.0 { raw + 1.0 } else { raw }
    } else {
        0.0
    };
    let s = if v > 0.0 { delta / v } else { 0.0 };
    h = (h + degrees / 360.0).rem_euclid(1.0);

    // HSV → RGB.
    let h6 = h * 6.0;
    let i = h6.floor();
    let f = h6 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Registry constructor.
pub(crate) fn from_params(params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    Box::new(ColorCorrect {
        brightness: param(params, "brightness", 0.0),
        contrast: param(params, "contrast", 0.0),
        saturation: param(params, "saturation", 1.0),
        hue_degrees: param(params, "hue", 0.0),
    })
}
