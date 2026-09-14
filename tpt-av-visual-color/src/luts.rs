//! 1D and 3D LUT loading and application.
//!
//! Supports the industry-standard `.cube` text format (Adobe/IRIDAS) for both
//! 1D (`LUT_1D_SIZE`) and 3D (`LUT_3D_SIZE`) tables. 3D tables are sampled
//! with trilinear interpolation.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::Result;
use crate::VisualError;

/// A 3D lookup table mapping RGB → RGB over the unit cube.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lut3D {
    /// LUT size per axis (`N` means `N³` entries).
    pub size: usize,
    /// Flat table, index `r + g*size + b*size²`, linear-domain RGB entries.
    pub data: Vec<[f32; 3]>,
}

/// A 1D lookup table with per-channel curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lut1D {
    /// Number of entries per channel.
    pub size: usize,
    /// Red curve.
    pub red: Vec<f32>,
    /// Green curve.
    pub green: Vec<f32>,
    /// Blue curve.
    pub blue: Vec<f32>,
}

impl Lut3D {
    /// Builds a LUT from a flat table, validating the length.
    pub fn new(size: usize, data: Vec<[f32; 3]>) -> Result<Self> {
        if size < 2 {
            return Err(VisualError::InvalidOperation(
                "3D LUT size must be at least 2".into(),
            ));
        }
        if data.len() != size * size * size {
            return Err(VisualError::InvalidOperation(format!(
                "3D LUT expects {} entries for size {size}, got {}",
                size * size * size,
                data.len()
            )));
        }
        Ok(Lut3D { size, data })
    }

    /// The identity LUT: sampling returns the input unchanged (within
    /// interpolation error).
    pub fn identity(size: usize) -> Result<Self> {
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let n = (size - 1) as f32;
                    data.push([
                        r as f32 / n,
                        g as f32 / n,
                        b as f32 / n,
                    ]);
                }
            }
        }
        Lut3D::new(size, data)
    }

    /// Trilinear interpolation over the unit cube.
    #[must_use]
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size as f32;
        // Coordinate in table space (0..size-1).
        let c = [
            (rgb[0].clamp(0.0, 1.0) * (n - 1.0)),
            (rgb[1].clamp(0.0, 1.0) * (n - 1.0)),
            (rgb[2].clamp(0.0, 1.0) * (n - 1.0)),
        ];
        let i: [usize; 3] = [
            (c[0] as usize).min(self.size - 2),
            (c[1] as usize).min(self.size - 2),
            (c[2] as usize).min(self.size - 2),
        ];
        let f: [f32; 3] = [c[0] - i[0] as f32, c[1] - i[1] as f32, c[2] - i[2] as f32];

        let at = |x: usize, y: usize, z: usize| -> [f32; 3] {
            self.data[x + y * self.size + z * self.size * self.size]
        };
        let lerp3 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
        };

        let c00 = lerp3(at(i[0], i[1], i[2]), at(i[0] + 1, i[1], i[2]), f[0]);
        let c10 = lerp3(at(i[0], i[1] + 1, i[2]), at(i[0] + 1, i[1] + 1, i[2]), f[0]);
        let c01 = lerp3(at(i[0], i[1], i[2] + 1), at(i[0] + 1, i[1], i[2] + 1), f[0]);
        let c11 = lerp3(
            at(i[0], i[1] + 1, i[2] + 1),
            at(i[0] + 1, i[1] + 1, i[2] + 1),
            f[0],
        );
        let c0 = lerp3(c00, c10, f[1]);
        let c1 = lerp3(c01, c11, f[1]);
        lerp3(c0, c1, f[2])
    }
}

impl Lut1D {
    /// Builds a per-channel LUT, validating lengths.
    pub fn new(red: Vec<f32>, green: Vec<f32>, blue: Vec<f32>) -> Result<Self> {
        if red.is_empty() || green.len() != red.len() || blue.len() != red.len() {
            return Err(VisualError::InvalidOperation(
                "1D LUT channels must be non-empty and equal length".into(),
            ));
        }
        let size = red.len();
        Ok(Lut1D {
            size,
            red,
            green,
            blue,
        })
    }

    /// Applies the per-channel curves to one color.
    #[must_use]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let curves = [&self.red, &self.green, &self.blue];
        let n = (self.size - 1) as f32;
        let mut out = [0.0_f32; 3];
        for (ch, (curve, v)) in curves.iter().zip(rgb).enumerate() {
            let x = v.clamp(0.0, 1.0) * n;
            let i = (x as usize).min(self.size - 2);
            let t = x - i as f32;
            out[ch] = curve[i] + (curve[i + 1] - curve[i]) * t;
        }
        out
    }
}

/// A parsed `.cube` file: either a 1D or 3D table.
#[derive(Debug, Clone, PartialEq)]
pub enum Cube {
    /// A 3D table.
    Lut3D(Lut3D),
    /// A 1D table.
    Lut1D(Lut1D),
}

/// Parses `.cube` text (Adobe/IRIDAS format).
pub fn parse_cube(text: &str) -> Result<Cube> {
    let mut lut3d_size: Option<usize> = None;
    let mut lut1d_size: Option<usize> = None;
    let mut entries: Vec<[f32; 3]> = Vec::new();

    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("TITLE") {
            let _ = rest; // metadata is not used by the engine
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("LUT_3D_SIZE")
            .or_else(|| line.strip_prefix("LUT_3D_INPUT_SIZE"))
        {
            lut3d_size = Some(rest.trim().parse().map_err(|_| {
                cube_error(line_no, "invalid LUT_3D_SIZE value")
            })?);
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("LUT_1D_SIZE")
            .or_else(|| line.strip_prefix("LUT_1D_INPUT_SIZE"))
        {
            lut1d_size = Some(rest.trim().parse().map_err(|_| {
                cube_error(line_no, "invalid LUT_1D_SIZE value")
            })?);
            continue;
        }
        if line.starts_with("DOMAIN_") || line.starts_with("ORIGIN") || line.starts_with("LUT_ID") {
            continue; // unit-domain assumed
        }
        // Data line: "r g b" (3D) or "v" / "r g b" (1D).
        let mut nums = line.split_whitespace();
        let r: f32 = nums
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| cube_error(line_no, "malformed LUT data line"))?;
        let g = nums.next().and_then(|v| v.parse::<f32>().ok());
        let b = nums.next().and_then(|v| v.parse::<f32>().ok());
        if nums.next().is_some() {
            return Err(cube_error(line_no, "too many columns in LUT data line"));
        }
        match (g, b) {
            (Some(g), Some(b)) => entries.push([r, g, b]),
            _ => {
                // 1D single-column entry: replicate across channels.
                entries.push([r, r, r]);
            }
        }
    }

    if let Some(size) = lut3d_size {
        return Ok(Cube::Lut3D(Lut3D::new(size, entries)?));
    }
    if let Some(size) = lut1d_size {
        let expect3 = size * 3;
        if entries.len() == size {
            // Single table replicated per channel.
            let table: Vec<f32> = entries.iter().map(|e| e[0]).collect();
            return Ok(Cube::Lut1D(Lut1D::new(table.clone(), table.clone(), table)?));
        }
        if entries.len() == expect3 {
            // Three-column 1D layout: red points first, then green, then
            // blue.
            let take = |start: usize| -> Vec<f32> {
                (0..size).map(|i| entries[start + i][0]).collect()
            };
            return Ok(Cube::Lut1D(Lut1D::new(take(0), take(size), take(size * 2))?));
        }
        return Err(VisualError::InvalidOperation(format!(
            "1D LUT of size {size} needs {size} or {expect3} entries, got {}",
            entries.len()
        )));
    }
    Err(VisualError::InvalidOperation(
        "missing LUT_3D_SIZE or LUT_1D_SIZE header".into(),
    ))
}

fn cube_error(line: usize, msg: &str) -> VisualError {
    VisualError::InvalidOperation(format!(".cube line {}: {msg}", line + 1))
}

/// Loads and parses a `.cube` file from disk.
pub fn load_cube_file(path: impl AsRef<Path>) -> Result<Cube> {
    let text = std::fs::read_to_string(path)?;
    parse_cube(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn identity_lut_is_identity() {
        let lut = Lut3D::identity(17).unwrap();
        for rgb in [[0.0, 0.0, 0.0], [0.25, 0.5, 0.75], [1.0, 1.0, 1.0], [0.1, 0.9, 0.42]] {
            let out = lut.sample(rgb);
            for (o, i) in out.iter().zip(rgb) {
                assert!(close(*o, i), "{rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn corner_values_are_exact() {
        let lut = Lut3D::identity(5).unwrap();
        assert_eq!(lut.sample([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        assert_eq!(lut.sample([1.0, 1.0, 1.0]), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn lut_rejects_wrong_size() {
        assert!(Lut3D::new(2, vec![[0.0; 3]; 7]).is_err());
        assert!(Lut3D::new(2, vec![[0.0; 3]; 8]).is_ok());
        assert!(Lut3D::new(1, vec![[0.0; 3]; 1]).is_err());
    }

    #[test]
    fn one_dim_lut_applies_curves() {
        // 3-entry gamma-ish curve.
        let lut = Lut1D::new(
            vec![0.0, 0.25, 1.0],
            vec![0.0, 0.5, 1.0],
            vec![0.0, 0.75, 1.0],
        )
        .unwrap();
        // 3 entries map 0.0/0.5/1.0 exactly; 0.5 hits the middle entry.
        let out = lut.apply([0.5, 0.5, 0.5]);
        assert!(close(out[0], 0.25));
        assert!(close(out[1], 0.5));
        assert!(close(out[2], 0.75));
        // Quarter points interpolate.
        let out = lut.apply([0.25, 0.25, 0.25]);
        assert!(close(out[0], 0.125)); // mid of 0.0..0.25
        assert!(close(out[1], 0.25));
        assert!(close(out[2], 0.375));
        // Clamps out-of-range.
        assert_eq!(lut.apply([-1.0, 0.0, 2.0])[0], 0.0);
        assert_eq!(lut.apply([-1.0, 0.0, 2.0])[2], 1.0);
    }

    #[test]
    fn parses_3d_cube() {
        let text = "\
# demo LUT
TITLE \"Test\"
LUT_3D_SIZE 2
DOMAIN_MIN 0.0 0.0 0.0
DOMAIN_MAX 1.0 1.0 1.0
0 0 0
1 0 0
0 1 0
1 1 0
0 0 1
1 0 1
0 1 1
1 1 1
";
        let cube = parse_cube(text).unwrap();
        let Cube::Lut3D(lut) = cube else {
            panic!("expected 3D LUT");
        };
        assert_eq!(lut.size, 2);
        // Pure red input returns the red corner.
        let out = lut.sample([1.0, 0.0, 0.0]);
        assert_eq!(out, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn parses_1d_cube_single_column() {
        let text = "LUT_1D_SIZE 3\n0.0\n0.5\n1.0\n";
        let cube = parse_cube(text).unwrap();
        let Cube::Lut1D(lut) = cube else {
            panic!("expected 1D LUT");
        };
        assert_eq!(lut.size, 3);
        assert!(close(lut.apply([0.5, 0.5, 0.5])[0], 0.5));
        assert!(close(lut.apply([0.25, 0.25, 0.25])[0], 0.25));
    }

    #[test]
    fn parses_1d_cube_three_column() {
        let text = "LUT_1D_SIZE 2\n0.0 0.0 0.0\n0.2 0.5 0.8\n1.0 1.0 1.0\n0.9 0.9 0.95\n0.1 0.1 0.1\n0.3 0.3 0.3\n";
        let cube = parse_cube(text).unwrap();
        let Cube::Lut1D(lut) = cube else {
            panic!("expected 1D LUT");
        };
        assert_eq!(lut.size, 2);
        // Red curve = the first `size` entries' red channel: [0.0, 0.2].
        assert!(close(lut.apply([0.5, 0.5, 0.5])[0], 0.1));
        // Green curve = next `size` entries: [1.0, 0.9].
        assert!(close(lut.apply([0.5, 0.5, 0.5])[1], 0.95));
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse_cube("1 2 3").is_err(), "no size header");
        assert!(parse_cube("LUT_3D_SIZE x").is_err());
        assert!(parse_cube("LUT_3D_SIZE 2\n1 2").is_err(), "bad data line");
        assert!(
            parse_cube("LUT_3D_SIZE 2\n1 2 3\n4 5 6\n").is_err(),
            "wrong entry count"
        );
    }
}
