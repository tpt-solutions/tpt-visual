//! Blur effects: gaussian, box, and motion blur.

use super::param;
use crate::effect::{Effect, EffectParams, EffectPassDesc};
use std::collections::BTreeMap;

const BLUR_WGSL: &str = include_str!("../gpu_shaders/blur.wgsl");

/// Separable Gaussian blur. `radius` is in output pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianBlur {
    /// Blur radius in pixels (sigma ≈ radius / 2).
    pub radius: f32,
}

impl GaussianBlur {
    /// Creates a gaussian blur with the given radius.
    #[must_use]
    pub fn new(radius: f32) -> Self {
        GaussianBlur {
            radius: radius.max(0.0),
        }
    }
}

impl Effect for GaussianBlur {
    fn name(&self) -> &'static str {
        "gaussian_blur"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        let radius = self.radius.max(1.0);
        let sigma = radius / 2.0;
        let size = ((sigma * 3.0).ceil() as usize).max(1);
        let kernel: Vec<f32> = (0..=size * 2)
            .map(|i| {
                let x = (i as f32 - size as f32) / radius;
                (-0.5 * x * x).exp()
            })
            .collect();
        separable_pass(rgba, width, height, &kernel, 0);
        separable_pass(rgba, width, height, &kernel, 1);
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut horizontal = EffectPassDesc {
            shader_source: BLUR_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        horizontal.params.p0 = [self.radius, 0.0, 0.0, 0.0];
        horizontal.params.p1 = [1.0, 0.0, 0.0, 0.0];
        horizontal.params.mode = 0;

        let mut vertical = horizontal.clone();
        vertical.params.p1 = [0.0, 1.0, 0.0, 0.0];
        vec![horizontal, vertical]
    }
}

/// Separable box blur.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxBlur {
    /// Box half-width in pixels.
    pub radius: f32,
}

impl BoxBlur {
    /// Creates a box blur with the given radius.
    #[must_use]
    pub fn new(radius: f32) -> Self {
        BoxBlur {
            radius: radius.max(0.0),
        }
    }
}

impl Effect for BoxBlur {
    fn name(&self) -> &'static str {
        "box_blur"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        let radius = self.radius.max(1.0);
        let size = radius.ceil() as usize;
        let kernel = vec![1.0_f32; size * 2 + 1];
        separable_pass(rgba, width, height, &kernel, 0);
        separable_pass(rgba, width, height, &kernel, 1);
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let mut horizontal = EffectPassDesc {
            shader_source: BLUR_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        horizontal.params.p0 = [self.radius, 0.0, 0.0, 0.0];
        horizontal.params.p1 = [1.0, 0.0, 0.0, 0.0];
        horizontal.params.mode = 1;

        let mut vertical = horizontal.clone();
        vertical.params.p1 = [0.0, 1.0, 0.0, 0.0];
        vec![horizontal, vertical]
    }
}

/// Directional (motion) blur in a single pass.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionBlur {
    /// Blur direction in degrees (0 = horizontal).
    pub angle_degrees: f32,
    /// Blur length in pixels.
    pub length: f32,
}

impl MotionBlur {
    /// Creates a motion blur with the given angle and length.
    #[must_use]
    pub fn new(angle_degrees: f32, length: f32) -> Self {
        MotionBlur {
            angle_degrees,
            length: length.max(0.0),
        }
    }
}

impl Effect for MotionBlur {
    fn name(&self) -> &'static str {
        "motion_blur"
    }

    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32) {
        let rad = self.angle_degrees.to_radians();
        let (dy, dx) = rad.sin_cos(); // angle 0 = horizontal, matching GPU
        let length = self.length.max(1.0);
        let taps = (length.ceil() as i64).max(1);
        let src = rgba.to_vec();
        for y in 0..height {
            for x in 0..width {
                let mut acc = [0.0_f32; 4];
                for t in -taps / 2..=taps / 2 {
                    let f = t as f32 / taps as f32;
                    let sx = (x as f32 + dx * f * length).round() as i64;
                    let sy = (y as f32 + dy * f * length).round() as i64;
                    let s = super::sample_clamped(&src, width, height, sx, sy);
                    acc = [acc[0] + s[0], acc[1] + s[1], acc[2] + s[2], acc[3] + s[3]];
                }
                let n = (taps * 2 + 1) as f32;
                super::write_pixel(
                    rgba,
                    width,
                    x,
                    y,
                    [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n],
                );
            }
        }
    }

    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc> {
        let rad = self.angle_degrees.to_radians();
        let mut desc = EffectPassDesc {
            shader_source: BLUR_WGSL,
            params: EffectParams::new(width, height),
            curve_lut: None,
        };
        desc.params.p0 = [0.0, self.length, 0.0, 0.0];
        desc.params.p1 = [rad.cos(), rad.sin(), 0.0, 0.0];
        desc.params.mode = 2;
        vec![desc]
    }
}

/// One axis of a separable convolution, in place. `axis` 0 = horizontal,
/// 1 = vertical. Operates through an f32 scratch to avoid precision loss.
fn separable_pass(rgba: &mut [u8], width: u32, height: u32, kernel: &[f32], axis: u8) {
    let kernel_sum: f32 = kernel.iter().sum();
    let radius = (kernel.len() / 2) as i64;

    // Precompute the tap offsets for this axis.
    let offsets: Vec<(i64, i64)> = if axis == 0 {
        (-radius..=radius).map(|d| (d, 0)).collect()
    } else {
        (-radius..=radius).map(|d| (0, d)).collect()
    };

    let src = rgba.to_vec();
    let mut acc = vec![0.0_f32; (width * height * 4) as usize];

    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0_f32; 4];
            for (wi, &(ddx, ddy)) in offsets.iter().enumerate() {
                let s = super::sample_clamped(
                    &src,
                    width,
                    height,
                    i64::from(x) + ddx,
                    i64::from(y) + ddy,
                );
                let w = kernel[wi];
                sum = [
                    sum[0] + s[0] * w,
                    sum[1] + s[1] * w,
                    sum[2] + s[2] * w,
                    sum[3] + s[3] * w,
                ];
            }
            let base = ((y * width + x) * 4) as usize;
            acc[base..base + 4].copy_from_slice(&sum);
        }
    }

    for (dst, px) in rgba.chunks_exact_mut(4).zip(acc.chunks_exact(4)) {
        for (slot, v) in dst.iter_mut().zip(px) {
            *slot = ((v / kernel_sum).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Registry constructors.
pub(crate) fn from_params(name: &'static str, params: &BTreeMap<String, f32>) -> Box<dyn Effect> {
    match name {
        "gaussian_blur" => Box::new(GaussianBlur::new(param(params, "radius", 4.0))),
        "box_blur" => Box::new(BoxBlur::new(param(params, "radius", 4.0))),
        "motion_blur" => Box::new(MotionBlur::new(
            param(params, "angle", 0.0),
            param(params, "length", 8.0),
        )),
        _ => unreachable!("registry dispatch checked upstream"),
    }
}
