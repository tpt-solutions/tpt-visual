//! Transfer functions (electro-optical transfer / gamma curves).

use serde::{Deserialize, Serialize};

/// A transfer function mapping scene/display linear light (normalized
/// 0.0–1.0) to a code value and back.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TransferFunction {
    /// IEC 61966-2-1 sRGB curve.
    Srgb,
    /// No transfer function — linear light.
    Linear,
    /// SMPTE ST 2084 (PQ), used by HDR10. Code 1.0 == 10000 nits.
    Pq,
    /// ARIB STD-B67 (HLG), used by broadcast HDR.
    Hlg,
    /// Pure power gamma: code = linear^(1/g) on encode, linear = code^g on
    /// decode. `Gamma(2.2)` / `Gamma(2.4)` are common display gammas.
    Gamma(f32),
}

impl TransferFunction {
    /// Stable id used by the GPU shader's mode uniform.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        match self {
            TransferFunction::Srgb => 0,
            TransferFunction::Linear => 1,
            TransferFunction::Pq => 2,
            TransferFunction::Hlg => 3,
            TransferFunction::Gamma(_) => 4,
        }
    }

    /// The gamma exponent for [`TransferFunction::Gamma`], if any.
    #[must_use]
    pub const fn gamma(self) -> Option<f32> {
        match self {
            TransferFunction::Gamma(g) => Some(g),
            _ => None,
        }
    }

    /// Decodes a code value to normalized linear light.
    ///
    /// For PQ the result is normalized so that 1.0 == 10 000 nits; for HLG
    /// the result is the scene-linear signal in the nominal [0, 1] range.
    #[must_use]
    pub fn decode(self, code: f32) -> f32 {
        let v = code.max(0.0);
        match self {
            TransferFunction::Srgb => {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            TransferFunction::Linear => v,
            TransferFunction::Pq => {
                // SMPTE ST 2084. m1 = 2610/16384, m2 = 2523/4096*128,
                // c1 = 3424/4096, c2 = 2413/4096*32, c3 = 2392/4096*32.
                const M1: f32 = 0.159_301_76;
                const M2: f32 = 78.843_75;
                const C1: f32 = 0.835_937_5;
                const C2: f32 = 18.851_562;
                const C3: f32 = 18.687_5;
                let ep = v.powf(1.0 / M2);
                ((ep - C1).max(0.0) / (C2 - C3 * ep)).powf(1.0 / M1)
            }
            TransferFunction::Hlg => {
                // ARIB STD-B67 / ITU-R BT.2100 OETF^-1.
                const A: f32 = 0.178_832_77;
                const B: f32 = 1.0 - 4.0 * A; // 0.28466892
                const C: f32 = 0.559_910_73;
                if v <= 0.5 {
                    v * v / 3.0
                } else {
                    (((v - C) / A).exp() + B) / 12.0
                }
            }
            TransferFunction::Gamma(g) => v.powf(g),
        }
    }

    /// Encodes normalized linear light to a code value.
    #[must_use]
    pub fn encode(self, linear: f32) -> f32 {
        let v = linear.max(0.0);
        match self {
            TransferFunction::Srgb => {
                if v <= 0.003_130_8 {
                    12.92 * v
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                }
            }
            TransferFunction::Linear => v,
            TransferFunction::Pq => {
                const M1: f32 = 0.159_301_76;
                const M2: f32 = 78.843_75;
                const C1: f32 = 0.835_937_5;
                const C2: f32 = 18.851_562;
                const C3: f32 = 18.687_5;
                let y = v.powf(M1);
                ((C1 + C2 * y) / (1.0 + C3 * y)).powf(M2)
            }
            TransferFunction::Hlg => {
                const A: f32 = 0.178_832_77;
                const B: f32 = 1.0 - 4.0 * A;
                const C: f32 = 0.559_910_73;
                if v <= 1.0 / 12.0 {
                    (3.0 * v).sqrt()
                } else {
                    A * (12.0 * v - B).ln() + C
                }
            }
            TransferFunction::Gamma(g) => v.powf(1.0 / g),
        }
    }

    /// Convenience: decodes all three components of a color.
    #[must_use]
    pub fn decode3(self, code: [f32; 3]) -> [f32; 3] {
        [
            self.decode(code[0]),
            self.decode(code[1]),
            self.decode(code[2]),
        ]
    }

    /// Convenience: encodes all three components of a color.
    #[must_use]
    pub fn encode3(self, linear: [f32; 3]) -> [f32; 3] {
        [
            self.encode(linear[0]),
            self.encode(linear[1]),
            self.encode(linear[2]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn srgb_reference_values() {
        // Published anchor: sRGB encode(0.5) = 0.7353569830524495.
        assert!(close(TransferFunction::Srgb.encode(0.5), 0.735_357, 1e-5));
        assert!(close(TransferFunction::Srgb.decode(0.735_357), 0.5, 1e-5));
        // Linear segment: code 0.04045 decodes exactly to 0.0031308.
        assert!(close(
            TransferFunction::Srgb.decode(0.04045),
            0.003_130_8,
            1e-7
        ));
    }

    #[test]
    fn pq_reference_values() {
        // Cross-checked against the ST 2084 formula at double precision:
        // 100 nits -> 0.50808, 1000 nits -> 0.75183 (0.0203 -> 0.58069).
        assert!(close(TransferFunction::Pq.encode(0.01), 0.508_078, 1e-5));
        assert!(close(TransferFunction::Pq.encode(0.1), 0.751_827, 1e-5));
        assert!(close(TransferFunction::Pq.encode(0.020_3), 0.580_689, 1e-5));
        // Decode is the exact inverse.
        assert!(close(TransferFunction::Pq.decode(0.508_078), 0.01, 1e-5));
        // Range is [0, 1] == [0, 10000] nits. Black encodes to the f32
        // evaluation floor of the ST 2084 formula (~7e-7, i.e. < 0.001
        // nits) rather than exact zero.
        assert!(TransferFunction::Pq.encode(0.0) < 1e-5);
        assert!(close(TransferFunction::Pq.encode(1.0), 1.0, 1e-6));
    }

    #[test]
    fn hlg_reference_values() {
        // BT.2100 anchors: HLG(0.05) = sqrt(0.15), HLG(1/12) = 0.5,
        // HLG(0.18) = 0.67236 (double-precision cross-check).
        assert!(close(
            TransferFunction::Hlg.encode(0.05),
            0.387_298_33,
            1e-6
        ));
        assert!(close(TransferFunction::Hlg.encode(1.0 / 12.0), 0.5, 1e-6));
        assert!(close(TransferFunction::Hlg.encode(0.18), 0.672_358, 1e-5));
        assert!(close(TransferFunction::Hlg.decode(0.672_358), 0.18, 1e-5));
    }

    #[test]
    fn gamma_symmetric() {
        let g = TransferFunction::Gamma(2.4);
        for &v in &[0.0_f32, 0.1, 0.5, 0.9, 1.0] {
            assert!(close(g.decode(g.encode(v)), v, 1e-6));
        }
    }

    #[test]
    fn roundtrips() {
        for tf in [
            TransferFunction::Srgb,
            TransferFunction::Linear,
            TransferFunction::Pq,
            TransferFunction::Hlg,
            TransferFunction::Gamma(2.2),
        ] {
            for &v in &[1e-4_f32, 0.01, 0.18, 0.5, 0.9] {
                let back = tf.decode(tf.encode(v));
                assert!(
                    (back - v).abs() < v * 5e-3 + 1e-6,
                    "{tf:?}({v}) roundtripped to {back}"
                );
            }
        }
    }

    #[test]
    fn clamps_negative_input() {
        for tf in [
            TransferFunction::Srgb,
            TransferFunction::Pq,
            TransferFunction::Hlg,
        ] {
            assert!(tf.decode(-1.0) >= 0.0);
            assert!(tf.encode(-1.0) >= 0.0);
        }
    }

    #[test]
    fn vec_helpers() {
        let out = TransferFunction::Linear.decode3([0.1, 0.2, 0.3]);
        assert_eq!(out, [0.1, 0.2, 0.3]);
        let out = TransferFunction::Srgb.encode3([0.5, 0.5, 0.5]);
        assert!(out.iter().all(|v| close(*v, 0.735_357, 1e-5)));
    }
}
