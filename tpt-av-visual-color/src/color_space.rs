//! Color space definitions: primaries, white points, and RGB↔XYZ matrices.

use serde::{Deserialize, Serialize};

use crate::transfer::TransferFunction;

/// A set of RGB primaries plus a white point, in CIE xy chromaticity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Primaries {
    /// Red chromaticity.
    pub red: [f32; 2],
    /// Green chromaticity.
    pub green: [f32; 2],
    /// Blue chromaticity.
    pub blue: [f32; 2],
    /// White point chromaticity.
    pub white: [f32; 2],
}

impl Primaries {
    /// ITU-R BT.709 / sRGB primaries, D65 white.
    pub const REC709: Primaries = Primaries {
        red: [0.640, 0.330],
        green: [0.300, 0.600],
        blue: [0.150, 0.060],
        white: [0.312_7, 0.329_0],
    };

    /// ITU-R BT.2020 wide-gamut primaries, D65 white.
    pub const REC2020: Primaries = Primaries {
        red: [0.708, 0.292],
        green: [0.170, 0.797],
        blue: [0.131, 0.046],
        white: [0.312_7, 0.329_0],
    };

    /// DCI-P3 with D65 white (a.k.a. Display P3).
    pub const P3_D65: Primaries = Primaries {
        red: [0.680, 0.320],
        green: [0.265, 0.690],
        blue: [0.150, 0.060],
        white: [0.312_7, 0.329_0],
    };

    /// ACES AP1 primaries (ACEScg working space), ACES white ~D60.
    pub const ACES_AP1: Primaries = Primaries {
        red: [0.713, 0.293],
        green: [0.165, 0.830],
        blue: [0.128, 0.044],
        white: [0.321_68, 0.337_67],
    };

    /// ACES AP0 primaries (ACES2065-1 interchange space), ACES white ~D60.
    pub const ACES_AP0: Primaries = Primaries {
        red: [0.734_7, 0.265_3],
        green: [0.0, 1.0],
        blue: [0.000_1, -0.077],
        white: [0.321_68, 0.337_67],
    };

    /// Computes the RGB→XYZ matrix for these primaries (row-major 3x3).
    ///
    /// Derived from the chromaticities: each primary contributes
    /// `X = x/y, Y = 1, Z = (1-x-y)/y` as a *column* of the unscaled matrix,
    /// and the column scale factors `S = M⁻¹ · W` map the white point to
    /// itself (with Y = 1).
    #[must_use]
    pub fn rgb_to_xyz(&self) -> [[f32; 3]; 3] {
        fn xyz(p: [f32; 2]) -> [f32; 3] {
            [p[0] / p[1], 1.0, (1.0 - p[0] - p[1]) / p[1]]
        }
        let xr = xyz(self.red);
        let xg = xyz(self.green);
        let xb = xyz(self.blue);
        let w = xyz(self.white);

        // Columns are the primaries.
        let m = [
            [xr[0], xg[0], xb[0]],
            [xr[1], xg[1], xb[1]],
            [xr[2], xg[2], xb[2]],
        ];
        let s = apply3(&invert3(&m), w);
        [
            [m[0][0] * s[0], m[0][1] * s[1], m[0][2] * s[2]],
            [m[1][0] * s[0], m[1][1] * s[1], m[1][2] * s[2]],
            [m[2][0] * s[0], m[2][1] * s[1], m[2][2] * s[2]],
        ]
    }

    /// The XYZ→RGB matrix (inverse of [`Primaries::rgb_to_xyz`]).
    #[must_use]
    pub fn xyz_to_rgb(&self) -> [[f32; 3]; 3] {
        invert3(&self.rgb_to_xyz())
    }
}

/// Inverts a 3x3 row-major matrix. Panics only for exactly-singular input,
/// which cannot happen for valid primary sets.
#[must_use]
pub fn invert3(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv = 1.0 / det;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ]
}

/// Applies a 3x3 row-major matrix to a color.
#[must_use]
pub fn apply3(m: &[[f32; 3]; 3], c: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * c[0] + m[0][1] * c[1] + m[0][2] * c[2],
        m[1][0] * c[0] + m[1][1] * c[1] + m[1][2] * c[2],
        m[2][0] * c[0] + m[2][1] * c[1] + m[2][2] * c[2],
    ]
}

/// Color space definitions.
///
/// Each space couples a set of primaries with its conventional transfer
/// function; the transfer can be overridden per-`ColorPipeline` when a space
/// is used with a different encoding.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ColorSpace {
    /// ITU-R BT.709 (HD television; same primaries as sRGB).
    Rec709,
    /// ITU-R BT.2020 (UHD television, wide gamut).
    Rec2020,
    /// DCI-P3 primaries with D65 white (a.k.a. Display P3). The vast
    /// majority of "P3" content is D65; the DCI theater white (0.314,
    /// 0.351) can be expressed via `Custom` if ever needed.
    DciP3,
    /// sRGB (web, consumer displays).
    Srgb,
    /// Linear RGB with Rec.709 primaries — the compositor's working space.
    Linear,
    /// ACES AP1 working space (ACEScg), linear.
    Aces,
    /// Custom color space defined by primaries and white point.
    Custom {
        /// RGB primaries.
        primaries: [[f32; 2]; 3],
        /// White point chromaticity.
        white_point: [f32; 2],
    },
}

impl ColorSpace {
    /// The primaries and white point of this space.
    #[must_use]
    pub fn primaries(&self) -> Primaries {
        match self {
            ColorSpace::Rec709 | ColorSpace::Srgb | ColorSpace::Linear => Primaries::REC709,
            ColorSpace::Rec2020 => Primaries::REC2020,
            ColorSpace::DciP3 => Primaries::P3_D65,
            ColorSpace::Aces => Primaries::ACES_AP1,
            ColorSpace::Custom {
                primaries,
                white_point,
            } => Primaries {
                red: primaries[0],
                green: primaries[1],
                blue: primaries[2],
                white: *white_point,
            },
        }
    }

    /// The space's conventional transfer function.
    #[must_use]
    pub const fn default_transfer(&self) -> TransferFunction {
        match self {
            ColorSpace::Rec709 => TransferFunction::Gamma(2.4), // BT.1886 display
            ColorSpace::Srgb => TransferFunction::Srgb,
            ColorSpace::Linear | ColorSpace::Aces => TransferFunction::Linear,
            ColorSpace::Rec2020 => TransferFunction::Pq,
            ColorSpace::DciP3 => TransferFunction::Gamma(2.6), // DCI cinema
            ColorSpace::Custom { .. } => TransferFunction::Srgb,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn rec709_xyz_matrix_matches_published_values() {
        // Reference: Bruce Lindbloom's RGB/XYZ matrices (sRGB/BT.709, D65).
        let m = Primaries::REC709.rgb_to_xyz();
        let expected = [
            [0.412391, 0.357584, 0.180481],
            [0.212639, 0.715169, 0.072192],
            [0.019331, 0.119195, 0.950532],
        ];
        for (row, exp) in m.iter().zip(expected) {
            for (v, e) in row.iter().zip(exp) {
                assert!(close(*v, e, 1e-5), "{v} != {e}");
            }
        }
    }

    #[test]
    fn xyz_roundtrip_is_identity() {
        for p in [
            Primaries::REC709,
            Primaries::REC2020,
            Primaries::P3_D65,
            Primaries::ACES_AP1,
        ] {
            let fwd = p.rgb_to_xyz();
            let back = p.xyz_to_rgb();
            // M · M⁻¹ = I (check via applying to basis vectors).
            for (idx, basis) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                .iter()
                .enumerate()
            {
                let out = apply3(&back, apply3(&fwd, *basis));
                for (j, v) in out.iter().enumerate() {
                    let want = if j == idx { 1.0 } else { 0.0 };
                    assert!(close(*v, want, 1e-4), "{p:?}: {out:?}");
                }
            }
        }
    }

    #[test]
    fn white_maps_to_white_y() {
        // The white point must map to XYZ with Y = 1.
        for p in [Primaries::REC709, Primaries::REC2020, Primaries::ACES_AP1] {
            let m = p.rgb_to_xyz();
            let w = apply3(&m, [1.0, 1.0, 1.0]);
            assert!(close(w[1], 1.0, 1e-5), "{p:?}: Y = {}", w[1]);
        }
    }

    #[test]
    fn custom_space_carries_primaries() {
        let cs = ColorSpace::Custom {
            primaries: [[0.7, 0.3], [0.1, 0.8], [0.1, 0.05]],
            white_point: [0.3127, 0.329],
        };
        let p = cs.primaries();
        assert_eq!(p.red, [0.7, 0.3]);
        assert_eq!(p.white, [0.3127, 0.329]);
    }
}
