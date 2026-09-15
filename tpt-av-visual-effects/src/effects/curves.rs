//! Tone curves (RGB and per-luminance), baked to 256-entry LUTs.

use super::param;
use crate::effect::{Effect, EffectParams, EffectPassDesc};
use std::collections::BTreeMap;

const COLOR_CORRECT_WGSL: &str = include_str!("../gpu_shaders/color_correct.wgsl");

/// A single monotone tone curve defined by `(input, output)` control points
/// (each 0..1), linearly interpolated and baked into a 256-entry LUT.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Curve {
    /// Monotone control points, unsorted input allowed.
    pub points: Vec<(f32, f32)>,
}

impl Curve {
    /// The identity curve.
    #[must_use]
    pub fn identity() -> Self {
        Curve {
            points: vec![(0.0, 0.0), (1.0, 1.0)],
        }
    }

    /// Evaluates the curve at `x` (0..1).
    #[must_use]
    pub fn eval(&self, x: f32) -> f32 {
        if self.points.is_empty() {
            return x;
        }
        let mut pts = self.points.clone();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if x <= pts[0].0 {
            return pts[0].1;
        }
        if x >= pts[pts.len() - 1].0 {
            return pts[pts.len() - 1].1;
        }
        let right = pts.partition_point(|p| p.0 < x).max(1);
        let (x0, y0) = pts[right - 1];
        let (x1, y1) = pts[right];
        let t = (x - x0) / (x1 - x0).max(f32::EPSILON);
        y0 + (y1 - y0) * t
    }

    /// Bakes the curve into 256 RGBA8 LUT entries (RGB channels carry the
    /// curve; alpha stays 255).
    #[must_use]
    pub fn bake_lut(&self) -> Vec<[u8; 4]> {
        (0..256)
            .map(|i| {
                let x = i as f32 / 255.0;
                let v = (self.eval(x).clamp(0.0, 1.0) * 255.0).round() as u8;
                [v, v, v, 255]
            })
            .collect()
    }
}

/// Tone curve effect with independent RGB and luminance curves.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToneCurve {
    /// Per-channel (RGB) curve.
    pub rgb: Curve,
    /// Optional luminance curve applied after the RGB curve.
    pub luminance: Option<Curve>,
}

impl ToneCurve {
    /// A curve effect driven only by an RGB curve.
    #[must_use]
    pub fn rgb(points: Vec<(f32, f32)>) -> Self {
        ToneCurve {
            rgb: Curve { points },
            luminance: None,
        }
    }
}

impl Effect for ToneCurve {
    fn name(&self) -> &'static str {
        "curves"
    }

    fn apply_cpu(&self, rgba: &mut [u8], _width: u32, _height: u32) {
        let lut_r = self.rgb.bake_lut();
        for px in rgba.chunks_exact_mut(4) {
            for slot in px.iter_mut().take(3) {
                *slot = lut_r[*slot as usize][0];
            }
            if let Some(luma_curve) = &self.luminance {
                let l = 0.2126 * f32::from(px[0])
                    + 0.7152 * f32::from(px[1])
                    + 0.0722 * f32::from(px[2]);
                let target = (luma_curve.eval(l / 255.0) * 255.0).round();
                let ratio = target / l.max(1.0);
                for slot in px.iter_mut().take(3) {
                    *slot = (f32::from(*slot) * ratio).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        // The GPU path routes the baked curve through color_correct.wgsl's
        // curve mode (mode bit 1) with the 256x1 LUT texture attached.
        let mut desc = EffectPassDesc {
            shader_source: COLOR_CORRECT_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: Some(self.rgb.bake_lut()),
        };
        desc.params.mode = 2;
        vec![desc]
    }
}

/// Registry constructor: `curve_0x/curve_0y/curve_1x/...` keys.
pub(crate) fn from_params(params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    let mut points = Vec::new();
    for i in 0..16 {
        let x = param(params, &format!("curve_{i}x"), f32::NAN);
        if x.is_nan() {
            break;
        }
        let y = param(params, &format!("curve_{i}y"), x);
        points.push((x, y));
    }
    if points.len() < 2 {
        points = vec![(0.0, 0.0), (1.0, 1.0)];
    }
    Box::new(ToneCurve::rgb(points))
}
