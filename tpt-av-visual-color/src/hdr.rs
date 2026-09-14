//! HDR tone mapping operators.

use serde::{Deserialize, Serialize};

/// An HDR → SDR (or HDR → HDR) tone mapping operator, applied to normalized
/// linear light.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToneMapper {
    /// Reinhard tonemapping: `y = x / (1 + x)`, optionally scaled so the
    /// given input level maps to 0.8 output (a common SDR target).
    Reinhard,
    /// The ACES filmic curve (Narkowicz fit to the RRT+ODT): filmic
    /// shoulder/toe with a 16.29-stop input range.
    AcesFilmic,
    /// Custom monotone curve defined by `(input, output)` points, linearly
    /// interpolated between samples and clamped at the ends.
    Custom {
        /// Tone curve as a list of (input, output) points.
        curve: Vec<(f32, f32)>,
    },
}

impl ToneMapper {
    /// Stable id used by the GPU shader's mode uniform.
    #[must_use]
    pub const fn as_u32(&self) -> u32 {
        match self {
            ToneMapper::Reinhard => 1,
            ToneMapper::AcesFilmic => 2,
            ToneMapper::Custom { .. } => 3,
        }
    }

    /// Maps one channel of normalized linear light.
    #[must_use]
    pub fn map(&self, x: f32) -> f32 {
        match self {
            ToneMapper::Reinhard => {
                // Scale so linear 1.0 (the SDR reference white) lands near
                // 0.8, preserving headroom character of the operator.
                let scaled = x * 4.0;
                scaled / (1.0 + scaled)
            }
            ToneMapper::AcesFilmic => {
                // Narkowicz 2015, "ACES Filmic Tone Mapping Curve".
                let x = x.max(0.0);
                let num = x * (2.51 * x + 0.03);
                let den = x * (2.43 * x + 0.59) + 0.14;
                (num / den).clamp(0.0, 1.0)
            }
            ToneMapper::Custom { curve } => custom_curve(curve, x),
        }
    }
}

fn custom_curve(curve: &[(f32, f32)], x: f32) -> f32 {
    if curve.is_empty() {
        return x;
    }
    let mut sorted: Vec<&(f32, f32)> = curve.iter().collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    if x <= sorted[0].0 {
        return sorted[0].1;
    }
    if let Some(last) = sorted.last() {
        if x >= last.0 {
            return last.1;
        }
    }
    let right = sorted.partition_point(|p| p.0 < x).max(1);
    let (x0, y0) = sorted[right - 1];
    let (x1, y1) = sorted[right];
    let t = (x - x0) / (x1 - x0).max(f32::EPSILON);
    y0 + (y1 - y0) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reinhard_maps_reference_white_below_one() {
        let tm = ToneMapper::Reinhard;
        assert_eq!(tm.map(0.0), 0.0);
        // x=1 → 4/5 = 0.8 target.
        assert!((tm.map(1.0) - 0.8).abs() < 1e-6);
        // Monotone and bounded.
        assert!(tm.map(100.0) < 1.0);
        assert!(tm.map(100.0) > tm.map(10.0));
    }

    #[test]
    fn aces_filmic_matches_reference_fits() {
        let tm = ToneMapper::AcesFilmic;
        // Narkowicz curve anchors (double-precision reference):
        assert!((tm.map(0.0) - 0.0).abs() < 1e-6);
        assert!((tm.map(0.5) - 0.616_306_95).abs() < 1e-5);
        assert!((tm.map(1.0) - 0.803_797_47).abs() < 1e-5);
        // Ten stops above white saturates.
        assert!((tm.map(10.0) - 1.0).abs() < 1e-5);
        // Clamped, never NaN.
        assert!((tm.map(1000.0) - 1.0).abs() <= f32::EPSILON);
    }

    #[test]
    fn custom_curve_interpolates_monotone() {
        let tm = ToneMapper::Custom {
            curve: vec![(0.0, 0.0), (1.0, 0.5), (4.0, 1.0)],
        };
        assert_eq!(tm.map(-1.0), 0.0);
        assert_eq!(tm.map(0.0), 0.0);
        assert!((tm.map(0.5) - 0.25).abs() < 1e-6);
        assert!((tm.map(2.5) - 0.75).abs() < 1e-6);
        assert_eq!(tm.map(4.0), 1.0);
        assert_eq!(tm.map(99.0), 1.0, "clamped above the last point");
    }

    #[test]
    fn custom_curve_unsorted_input() {
        let tm = ToneMapper::Custom {
            curve: vec![(4.0, 1.0), (0.0, 0.0), (1.0, 0.5)],
        };
        assert!((tm.map(0.5) - 0.25).abs() < 1e-6, "sorts internally");
    }

    #[test]
    fn empty_custom_curve_is_identity() {
        let tm = ToneMapper::Custom { curve: vec![] };
        assert_eq!(tm.map(0.42), 0.42);
    }
}
