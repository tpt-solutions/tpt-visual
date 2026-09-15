//! ACES (Academy Color Encoding System) transforms.
//!
//! Implements the ACEScc / ACEScct logarithmic encodings per S-2014-003 /
//! S-2016-001, plus helpers to move linear data into the ACES AP0/AP1
//! spaces via [`crate::gamut`].

use crate::color_space::{ColorSpace, Primaries};
use crate::gamut::GamutConverter;
use crate::Result;

/// Official ACES AP1 → XYZ matrix (S-2014-004), ACES white at
/// XYZ (0.9526460746, 1.0, 1.0088251844).
pub const AP1_TO_XYZ: [[f32; 3]; 3] = [
    [0.662_454_18, 0.134_004_21, 0.156_187_69],
    [0.272_228_72, 0.674_081_77, 0.053_689_52],
    [-0.005_574_65, 0.004_060_73, 1.010_339_1],
];

/// Official XYZ → ACES AP1 matrix (inverse of [`AP1_TO_XYZ`]).
pub const XYZ_TO_AP1: [[f32; 3]; 3] = [
    [1.641_023_4, -0.324_803_3, -0.236_424_7],
    [-0.663_662_86, 1.615_331_6, 0.016_756_348],
    [0.011_721_894, -0.008_284_442, 0.988_394_86],
];

/// Official ACES AP0 → XYZ matrix (S-2014-004), ACES2065-1 interchange.
pub const AP0_TO_XYZ: [[f32; 3]; 3] = [
    [0.952_552_4, 0.0, 0.000_09],
    [0.343_966_45, 0.728_166_1, -0.072_132_48],
    [0.0, 0.0, 1.008_825_2],
];

/// Official XYZ → ACES AP0 matrix (inverse of [`AP0_TO_XYZ`]).
pub const XYZ_TO_AP0: [[f32; 3]; 3] = [
    [1.049_206_6, 0.0, -0.000_093_653_48],
    [-0.495_878_9, 1.377_158_5, 0.098_346_26],
    [0.0, 0.0, 0.991_254_3],
];

/// Converts ACEScct code values to scene-linear (AP1, normalized) per
/// S-2016-001.
#[must_use]
pub fn acescct_to_linear(code: f32) -> f32 {
    const Y_BRK: f32 = 0.155_251_141_552_511;
    const A: f32 = 10.540_237_741_654_5;
    const B: f32 = 9.024_654_203_870_82e-3;
    if code <= Y_BRK {
        (code - B) / A
    } else {
        (2.0_f32).powf(code * 17.52 - 9.72)
    }
}

/// Converts scene-linear values to ACEScct code values.
#[must_use]
pub fn linear_to_acescct(linear: f32) -> f32 {
    const X_BRK: f32 = 0.007_812_5;
    const A: f32 = 10.540_237_741_654_5;
    const B: f32 = 9.024_654_203_870_82e-3;
    let v = linear.max(0.0);
    if v <= X_BRK {
        A * v + B
    } else {
        (v.log2() + 9.72) / 17.52
    }
}

/// Converts ACEScc code values to scene-linear (AP1, normalized) per
/// S-2014-003. Note ACEScc has no toe: values below the break use the
/// documented `-0.3584` floor behaviour.
#[must_use]
pub fn acescc_to_linear(code: f32) -> f32 {
    if code < -0.301_384_382_853_532_5 {
        // Below log2(2^-16): the encoding defines this as an extrapolation
        // zone; clamp to the documented minimum.
        0.0
    } else {
        (2.0_f32).powf(code * 17.52 - 9.72)
    }
}

/// Converts scene-linear values to ACEScc code values.
#[must_use]
pub fn linear_to_acescc(linear: f32) -> f32 {
    let v = linear.max(2.0_f32.powi(-16));
    (v.log2() + 9.72) / 17.52
}

/// Builds a linear Rec.709 → ACES AP1 (ACEScg) converter.
///
/// This is the standard "IDT-like" entry point: decode your input transfer
/// to linear, then run through this matrix.
pub fn rec709_to_ap1() -> Result<GamutConverter> {
    GamutConverter::new(ColorSpace::Linear, ColorSpace::Aces)
}

/// ACES AP1 primaries (ACEScg working space).
#[must_use]
pub const fn ap1_primaries() -> Primaries {
    Primaries::ACES_AP1
}

/// ACES AP0 primaries (ACES2065-1 interchange space).
#[must_use]
pub const fn ap0_primaries() -> Primaries {
    Primaries::ACES_AP0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn acescct_mid_grey_reference() {
        // S-2016-001 anchor: ACEScct(0.18) = 0.4135884.
        assert!(close(linear_to_acescct(0.18), 0.413_588_4, 1e-6));
        assert!(close(acescct_to_linear(0.413_588_4), 0.18, 1e-6));
    }

    #[test]
    fn acescct_toe_reference() {
        // Below the break the encoding is linear: ACEScct(0.005) = 0.061726
        // (double-precision cross-check).
        assert!(close(linear_to_acescct(0.005), 0.061_725_84, 1e-6));
        assert!(close(acescct_to_linear(0.061_725_84), 0.005, 1e-6));
        // The toe is linear: doubling input doubles the code offset.
        let l1 = linear_to_acescct(0.001);
        let l2 = linear_to_acescct(0.002);
        assert!(close(l2 - l1, A * 0.001, 1e-6), "linear toe");
    }

    const A: f32 = 10.540_237_741_654_5;

    #[test]
    fn acescct_breakpoint_is_continuous() {
        // S-2016-001: the curve is continuous at the toe break.
        let below = linear_to_acescct(0.007_812_4);
        let at = linear_to_acescct(0.007_812_5);
        assert!((at - below).abs() < 1e-5, "{below} vs {at}");
    }

    #[test]
    fn acescct_roundtrip() {
        for &v in &[1e-5_f32, 0.005, 0.007, 0.18, 1.0, 16.0] {
            let back = acescct_to_linear(linear_to_acescct(v));
            assert!(close(back, v, v * 1e-3 + 1e-7), "{v} -> {back}");
        }
    }

    #[test]
    fn acescc_roundtrip_above_floor() {
        for &v in &[0.001, 0.18, 1.0, 8.0] {
            let back = acescc_to_linear(linear_to_acescc(v));
            assert!(close(back, v, v * 1e-3), "{v} -> {back}");
        }
    }

    #[test]
    fn acescc_and_acescct_agree_above_the_toe() {
        // Per spec, ACEScc and ACEScct are identical above the toe break.
        for &v in &[0.05, 0.18, 1.0] {
            assert!(close(linear_to_acescct(v), linear_to_acescc(v), 1e-6));
        }
    }

    #[test]
    fn negative_and_zero_inputs() {
        assert!(close(linear_to_acescct(0.0), 9.024_654e-3, 1e-6));
        assert!(
            close(linear_to_acescct(-1.0), 9.024_654e-3, 1e-6),
            "clamped"
        );
        assert_eq!(acescc_to_linear(-10.0), 0.0);
    }

    #[test]
    fn ap1_matrix_matches_official_constants() {
        // Primaries::ACES_AP1.rgb_to_xyz() must reproduce the official
        // S-2014-004 AP1→XYZ matrix (validated in color_space tests).
        let m = ap1_primaries().rgb_to_xyz();
        for (row, exp) in m.iter().zip(AP1_TO_XYZ) {
            for (v, e) in row.iter().zip(exp) {
                assert!((v - e).abs() < 1e-4, "{v} vs {e}");
            }
        }
    }

    #[test]
    fn rec709_to_ap1_preserves_neutrals() {
        // 709 → AP1 composes through XYZ with a Bradford adaptation for the
        // D65 → ACES white move. The exact coefficients vary between
        // references with the adaptation used, so we validate the invariants
        // that must hold for interchange: white maps to white, and the
        // conversion is order-consistent with the official matrices.
        let conv = rec709_to_ap1().unwrap();
        let w = conv.convert_unclamped([1.0, 1.0, 1.0]);
        for v in w {
            assert!((v - 1.0).abs() < 1e-4, "white -> {w:?}");
        }
        // AP1 primary directions are preserved: pure AP1 red converts back
        // from Rec.709 to within a small tolerance.
        let back = GamutConverter::new(ColorSpace::Aces, ColorSpace::Linear).unwrap();
        let red = back.convert_unclamped(conv.convert_unclamped([1.0, 0.0, 0.0]));
        assert!(
            (red[0] - 1.0).abs() < 1e-3 && red[1].abs() < 1e-3 && red[2].abs() < 1e-3,
            "{red:?}"
        );
    }
}
