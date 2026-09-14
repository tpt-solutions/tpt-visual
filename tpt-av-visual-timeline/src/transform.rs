//! Spatial transforms for clips.

use serde::{Deserialize, Serialize};

/// Spatial transform of a clip: position, scale, rotation about an anchor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Position (x, y) in canvas pixels — where the anchor lands.
    pub position: (f32, f32),
    /// Scale factor per axis (1.0 = 100%).
    pub scale: (f32, f32),
    /// Rotation in degrees, clockwise.
    pub rotation: f32,
    /// Anchor point (rotation/scale center) in normalized clip coordinates
    /// (0.0–1.0); (0.5, 0.5) is the clip center.
    pub anchor: (f32, f32),
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation: 0.0,
            anchor: (0.5, 0.5),
        }
    }
}

impl Transform {
    /// The identity transform (no position offset, 100% scale, no rotation).
    pub const IDENTITY: Transform = Transform {
        position: (0.0, 0.0),
        scale: (1.0, 1.0),
        rotation: 0.0,
        anchor: (0.5, 0.5),
    };

    /// Builds the 3x2 affine transform (row-major, `[a b tx; c d ty]`) that
    /// maps normalized clip coordinates (0–1, origin top-left) into canvas
    /// pixels for a clip of `clip_size` on a canvas of `canvas_size`.
    ///
    /// Order: translate to anchor → rotate → scale → un-translate anchor →
    /// translate to position. The compositor's transform node inverts this
    /// matrix to pull-sample the source.
    #[must_use]
    pub fn to_matrix(self, clip_size: (f32, f32), canvas_size: (f32, f32)) -> Matrix2x3 {
        let (cw, ch) = clip_size;
        let anchor_px = (self.anchor.0 * cw, self.anchor.1 * ch);
        let rad = self.rotation.to_radians();
        let (s, c) = rad.sin_cos();
        // rotate (clockwise in screen coords, y-down) then scale
        let a = c * self.scale.0;
        let b = -s * self.scale.0;
        let cc = s * self.scale.1;
        let d = c * self.scale.1;
        // canvas offset: center the clip, then apply the user position.
        let base = (
            (canvas_size.0 - cw) * 0.5 + self.position.0,
            (canvas_size.1 - ch) * 0.5 + self.position.1,
        );
        // M = T(base + anchor) * RS * T(-anchor)
        let tx = base.0 + anchor_px.0 - (a * anchor_px.0 + cc * anchor_px.1);
        let ty = base.1 + anchor_px.1 - (b * anchor_px.0 + d * anchor_px.1);
        Matrix2x3 {
            a,
            b,
            tx,
            c: cc,
            d,
            ty,
        }
    }
}

/// A 2D affine matrix in row-major `[a b tx; c d ty]` layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix2x3 {
    /// Row 0, column 0.
    pub a: f32,
    /// Row 0, column 1.
    pub b: f32,
    /// Row 0 translation.
    pub tx: f32,
    /// Row 1, column 0.
    pub c: f32,
    /// Row 1, column 1.
    pub d: f32,
    /// Row 1 translation.
    pub ty: f32,
}

impl Matrix2x3 {
    /// The identity matrix.
    pub const IDENTITY: Matrix2x3 = Matrix2x3 {
        a: 1.0,
        b: 0.0,
        tx: 0.0,
        c: 0.0,
        d: 1.0,
        ty: 0.0,
    };

    /// Applies the matrix to a point.
    #[must_use]
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.b * y + self.tx,
            self.c * x + self.d * y + self.ty,
        )
    }

    /// Inverts the matrix. Returns `None` for singular (zero-scale) matrices.
    #[must_use]
    pub fn inverse(&self) -> Option<Matrix2x3> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < f32::EPSILON {
            return None;
        }
        let inv = 1.0 / det;
        Some(Matrix2x3 {
            a: self.d * inv,
            b: -self.b * inv,
            c: -self.c * inv,
            d: self.a * inv,
            tx: (self.b * self.ty - self.d * self.tx) * inv,
            ty: (self.c * self.tx - self.a * self.ty) * inv,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_maps_center_to_center() {
        let m = Transform::IDENTITY.to_matrix((100.0, 50.0), (1920.0, 1080.0));
        let (x, y) = m.apply(50.0, 25.0);
        assert!((x - 960.0).abs() < 1e-3);
        assert!((y - 540.0).abs() < 1e-3);
    }

    #[test]
    fn position_moves_anchor_landing_point() {
        let t = Transform {
            position: (100.0, -50.0),
            ..Transform::IDENTITY
        };
        // Clip 100x100 on a 1000x1000 canvas: centered base = (450, 450);
        // the user position shifts it to (550, 400). The clip center (the
        // anchor) lands at base + center = (600, 450).
        let m = t.to_matrix((100.0, 100.0), (1000.0, 1000.0));
        let (x, y) = m.apply(50.0, 50.0); // clip center
        assert!((x - 600.0).abs() < 1e-3);
        assert!((y - 450.0).abs() < 1e-3);
    }

    #[test]
    fn inverse_roundtrips() {
        let t = Transform {
            position: (35.0, -12.0),
            scale: (1.5, 0.75),
            rotation: 30.0,
            anchor: (0.25, 0.75),
        };
        let m = t.to_matrix((320.0, 240.0), (1920.0, 1080.0));
        let inv = m.inverse().expect("invertible");
        let (x, y) = m.apply(123.0, 45.0);
        let (x2, y2) = inv.apply(x, y);
        assert!((x2 - 123.0).abs() < 1e-2);
        assert!((y2 - 45.0).abs() < 1e-2);
    }

    #[test]
    fn rotation_about_anchor_keeps_anchor_fixed() {
        // Anchor at clip corner (0,0 normalized top-left): rotating must keep
        // that point at its landing spot.
        let t = Transform {
            rotation: 90.0,
            anchor: (0.0, 0.0),
            ..Transform::IDENTITY
        };
        let clip = (100.0, 100.0);
        let canvas = (100.0, 100.0);
        let m = t.to_matrix(clip, canvas);
        let (x, y) = m.apply(0.0, 0.0);
        // base = (0,0); anchor = (0,0) → point lands at (0,0).
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4);
        // Clockwise rotation: bottom edge points left, right edge points down.
        let (x2, y2) = m.apply(0.0, 100.0);
        assert!((x2 + 100.0).abs() < 1e-4, "x2 = {x2}");
        assert!(y2.abs() < 1e-4, "y2 = {y2}");
        let (x3, y3) = m.apply(100.0, 0.0);
        assert!(x3.abs() < 1e-4, "x3 = {x3}");
        assert!((y3 - 100.0).abs() < 1e-4, "y3 = {y3}");
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        let t = Transform {
            scale: (0.0, 0.0),
            ..Transform::IDENTITY
        };
        let m = t.to_matrix((100.0, 100.0), (100.0, 100.0));
        assert!(m.inverse().is_none());
    }
}
