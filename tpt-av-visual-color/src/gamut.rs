//! Gamut mapping and conversion (Rec.709 ↔ Rec.2020 ↔ P3 ↔ AP1).

use crate::color_space::{apply3, invert3, ColorSpace};
use crate::Result;
use crate::VisualError;
use serde::{Deserialize, Serialize};

const IDENTITY3: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// 3x3 row-major matrix multiply.
fn mul3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0_f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// White point chromaticity → Y = 1 XYZ tristimulus.
fn white_xyz(xy: [f32; 2]) -> [f32; 3] {
    [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
}

/// Bradford chromatic adaptation matrix mapping `src_white` XYZ onto
/// `dst_white` XYZ (both Y = 1).
#[must_use]
pub fn bradford(src_white: [f32; 3], dst_white: [f32; 3]) -> [[f32; 3]; 3] {
    const M_A: [[f32; 3]; 3] = [
        [0.895_1, 0.266_4, -0.161_4],
        [-0.750_2, 1.713_5, 0.036_7],
        [0.038_9, -0.068_5, 1.029_6],
    ];
    let m_a_inv = invert3(&M_A);
    let s = apply3(&M_A, src_white);
    let d = apply3(&M_A, dst_white);
    // D = diag(d / s)
    let mut scaled = [[0.0_f32; 3]; 3];
    for i in 0..3 {
        scaled[i][i] = d[i] / s[i];
    }
    mul3(&m_a_inv, &mul3(&scaled, &M_A))
}

/// Converts linear RGB between color spaces through CIE XYZ.
///
/// Values outside the destination gamut are **clamped**, not projected —
/// camera-negative-style soft clipping is future work (see DESIGN.md).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GamutConverter {
    src: ColorSpace,
    dst: ColorSpace,
    /// `dst_to_rgb · xyz(src → xyz dst)` composed once.
    src_rgb_to_dst_rgb: [[f32; 3]; 3],
}

impl GamutConverter {
    /// Builds the conversion matrix for `src → dst` linear RGB.
    ///
    /// When the two spaces have different white points, a Bradford chromatic
    /// adaptation is composed so that neutrals stay neutral (matching the
    /// behaviour of ACES input transforms and OCIO display configs).
    pub fn new(src: ColorSpace, dst: ColorSpace) -> Result<Self> {
        if src == dst {
            return Ok(GamutConverter {
                src,
                dst,
                src_rgb_to_dst_rgb: IDENTITY3,
            });
        }
        let src_prim = src.primaries();
        let dst_prim = dst.primaries();
        let to_xyz = src_prim.rgb_to_xyz();
        let from_xyz = invert3(&dst_prim.rgb_to_xyz());

        // Bradford adaptation src white → dst white (Y = 1 XYZ values).
        let adapt = if src_prim.white == dst_prim.white {
            IDENTITY3
        } else {
            bradford(white_xyz(src_prim.white), white_xyz(dst_prim.white))
        };

        // m = from_xyz · adapt · to_xyz
        let mid = mul3(&adapt, &to_xyz);
        let m = mul3(&from_xyz, &mid);
        if m.iter().any(|r| r.iter().any(|v| !v.is_finite())) {
            return Err(VisualError::InvalidOperation(format!(
                "degenerate gamut conversion {src:?} -> {dst:?}"
            )));
        }
        Ok(GamutConverter {
            src,
            dst,
            src_rgb_to_dst_rgb: m,
        })
    }

    /// The source space.
    #[must_use]
    pub const fn source(&self) -> ColorSpace {
        self.src
    }

    /// The destination space.
    #[must_use]
    pub const fn destination(&self) -> ColorSpace {
        self.dst
    }

    /// The composed linear-RGB conversion matrix (row-major).
    #[must_use]
    pub const fn matrix(&self) -> [[f32; 3]; 3] {
        self.src_rgb_to_dst_rgb
    }

    /// Converts one linear RGB color.
    #[must_use]
    pub fn convert(&self, rgb: [f32; 3]) -> [f32; 3] {
        let out = apply3(&self.src_rgb_to_dst_rgb, rgb);
        out.map(|v| v.clamp(0.0, 1.0))
    }

    /// Converts linear RGB without clamping (for HDR workflows where
    /// out-of-gamut values are legal intermediate data).
    #[must_use]
    pub fn convert_unclamped(&self, rgb: [f32; 3]) -> [f32; 3] {
        apply3(&self.src_rgb_to_dst_rgb, rgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn rec709_to_rec2020_matches_published_matrix() {
        // Reference matrix (Lindbloom / OpenColorIO BT.709→BT.2020 transform).
        let conv = GamutConverter::new(ColorSpace::Linear, ColorSpace::Rec2020).unwrap();
        let m = conv.matrix();
        let expected = [
            [0.627404, 0.329283, 0.043313],
            [0.069097, 0.919540, 0.011362],
            [0.016391, 0.088013, 0.895595],
        ];
        for (row, exp) in m.iter().zip(expected) {
            for (v, e) in row.iter().zip(exp) {
                assert!(close(*v, e, 1e-4), "{v} != {e}");
            }
        }
    }

    #[test]
    fn rec709_to_p3_matches_published_matrix() {
        let conv = GamutConverter::new(ColorSpace::Linear, ColorSpace::DciP3).unwrap();
        let m = conv.matrix();
        let expected = [
            [0.822462, 0.177538, 0.000000],
            [0.033194, 0.966806, 0.000000],
            [0.017083, 0.072397, 0.910520],
        ];
        for (row, exp) in m.iter().zip(expected) {
            for (v, e) in row.iter().zip(exp) {
                assert!(close(*v, e, 2e-4), "{v} != {e}");
            }
        }
    }

    #[test]
    fn neutral_axis_stays_neutral() {
        // Equal-energy conversions must keep greys grey (matching white
        // points). This is the core "does it look right" invariant.
        for (src, dst) in [
            (ColorSpace::Linear, ColorSpace::Rec2020),
            (ColorSpace::Rec2020, ColorSpace::Linear),
            (ColorSpace::Linear, ColorSpace::DciP3),
            (ColorSpace::DciP3, ColorSpace::Rec2020),
        ] {
            let conv = GamutConverter::new(src, dst).unwrap();
            for &g in &[0.0_f32, 0.18, 0.5, 1.0] {
                let out = conv.convert([g, g, g]);
                for v in out {
                    assert!(close(v, g, 1e-4), "{src:?}->{dst:?}: grey {g} -> {out:?}");
                }
            }
        }
    }

    #[test]
    fn roundtrip_recovers_input_inside_gamut() {
        let fwd = GamutConverter::new(ColorSpace::Linear, ColorSpace::Rec2020).unwrap();
        let rev = GamutConverter::new(ColorSpace::Rec2020, ColorSpace::Linear).unwrap();
        for rgb in [[0.2, 0.5, 0.8], [0.7, 0.1, 0.3], [0.18, 0.18, 0.18]] {
            let out = rev.convert(fwd.convert(rgb));
            for (v, e) in out.iter().zip(rgb) {
                assert!(close(*v, e, 1e-4), "{rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn same_space_is_identity() {
        let conv = GamutConverter::new(ColorSpace::Aces, ColorSpace::Aces).unwrap();
        assert_eq!(conv.convert([0.3, 0.6, 0.9]), [0.3, 0.6, 0.9]);
    }

    #[test]
    fn out_of_gamut_clamps() {
        // Pure Rec.2020 green is far outside Rec.709; clamped conversion
        // must not produce negative values.
        let conv = GamutConverter::new(ColorSpace::Rec2020, ColorSpace::Linear).unwrap();
        let out = conv.convert([0.0, 1.0, 0.0]);
        assert!(out.iter().all(|v| *v >= 0.0), "{out:?}");
        let unc = conv.convert_unclamped([0.0, 1.0, 0.0]);
        assert!(unc[0] < 0.0, "unclamped should go negative: {unc:?}");
    }
}
