//! Keyframe animation system.
//!
//! A [`KeyframeTrack`] animates a single scalar property over time.
//! Interpolation between keyframes is linear, cubic (Catmull-Rom), or cubic
//! bezier easing (with per-keyframe control points).

use serde::{Deserialize, Serialize};

/// How values between keyframes are interpolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InterpolationMethod {
    /// Straight-line interpolation.
    #[default]
    Linear,
    /// Cubic interpolation through neighbouring points (Catmull-Rom).
    Cubic,
    /// Cubic bezier easing, using each keyframe's `bezier` control points.
    Bezier,
}

/// A single keyframe point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Time position, in session frames.
    pub frame: u64,
    /// Value at this point.
    pub value: f32,
    /// Bezier control points `(x1, y1, x2, y2)` in the unit square, mapping
    /// normalized time → normalized progress (CSS-easing style). Only used by
    /// [`InterpolationMethod::Bezier`].
    pub bezier: Option<(f32, f32, f32, f32)>,
}

impl Keyframe {
    /// A linear keyframe.
    #[must_use]
    pub fn at(frame: u64, value: f32) -> Self {
        Keyframe {
            frame,
            value,
            bezier: None,
        }
    }

    /// A keyframe with bezier easing control points.
    #[must_use]
    pub fn bezier(frame: u64, value: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Keyframe {
            frame,
            value,
            bezier: Some((x1, y1, x2, y2)),
        }
    }
}

/// A keyframe animation track for a single scalar property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyframeTrack {
    /// Property being animated, e.g. `"transform.position.y"` or
    /// `"effects.0.opacity"`.
    pub property: String,
    /// Keyframe points, kept sorted by frame.
    pub keyframes: Vec<Keyframe>,
    /// Interpolation method for the whole track.
    pub interpolation: InterpolationMethod,
}

impl KeyframeTrack {
    /// Creates an empty track for `property`.
    #[must_use]
    pub fn new(property: impl Into<String>, interpolation: InterpolationMethod) -> Self {
        KeyframeTrack {
            property: property.into(),
            keyframes: Vec::new(),
            interpolation,
        }
    }

    /// Whether the track animates anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keyframes.is_empty()
    }

    /// Adds a keyframe, keeping the list sorted by frame. Replaces an
    /// existing keyframe at the same frame.
    pub fn upsert_keyframe(&mut self, keyframe: Keyframe) {
        match self
            .keyframes
            .binary_search_by(|k| k.frame.cmp(&keyframe.frame))
        {
            Ok(idx) => self.keyframes[idx] = keyframe,
            Err(idx) => self.keyframes.insert(idx, keyframe),
        }
    }

    /// Evaluates the animated value at `frame`.
    ///
    /// An empty track evaluates to `0.0`; frames before the first / after the
    /// last keyframe hold the boundary value (clamped).
    #[must_use]
    pub fn evaluate(&self, frame: u64) -> f32 {
        let kfs = &self.keyframes;
        match kfs.len() {
            0 => return 0.0,
            1 => return kfs[0].value,
            _ => {}
        }
        if frame <= kfs[0].frame {
            return kfs[0].value;
        }
        if frame >= kfs[kfs.len() - 1].frame {
            return kfs[kfs.len() - 1].value;
        }
        let right = kfs.partition_point(|k| k.frame <= frame).max(1);
        let a = &kfs[right - 1];
        let b = &kfs[right];
        let span = (b.frame - a.frame).max(1) as f32;
        let raw_t = (frame - a.frame) as f32 / span;

        let eased_t = match self.interpolation {
            InterpolationMethod::Linear => raw_t,
            InterpolationMethod::Cubic => catmull_rom_t(kfs, right - 1, raw_t),
            InterpolationMethod::Bezier => {
                let (x1, y1, x2, y2) = a.bezier.unwrap_or((0.25, 0.1, 0.25, 1.0));
                cubic_bezier_ease(raw_t, x1, y1, x2, y2)
            }
        };
        a.value + (b.value - a.value) * eased_t
    }

    /// Samples all values between `start` and `end` (inclusive), useful for
    /// baking and previews.
    #[must_use]
    pub fn sample_range(&self, start: u64, end: u64) -> Vec<f32> {
        (start..=end).map(|f| self.evaluate(f)).collect()
    }
}

/// Cubic interpolation of the *progress* between `kfs[i]` and `kfs[i+1]`
/// using Catmull-Rom over the surrounding points (clamped at the ends).
fn catmull_rom_t(kfs: &[Keyframe], i: usize, t: f32) -> f32 {
    let p0 = kfs[i.saturating_sub(1)].value;
    let p1 = kfs[i].value;
    let p2 = kfs[i + 1].value;
    let p3 = kfs[(i + 2).min(kfs.len() - 1)].value;
    // Catmull-Rom spline on the VALUES, then renormalize the segment span so
    // the result stays monotone with t.
    let v = 0.5
        * ((2.0 * p1)
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
    // Convert the interpolated value back into segment progress.
    let span = p2 - p1;
    if span.abs() < f32::EPSILON {
        return 0.0;
    }
    ((v - p1) / span).clamp(0.0, 1.0)
}

/// Solves a CSS-style cubic bezier easing for `y(t)` at normalized time `x`.
fn cubic_bezier_ease(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    fn bezier_component(t: f32, p1: f32, p2: f32) -> f32 {
        let omt = 1.0 - t;
        3.0 * omt * omt * t * p1 + 3.0 * omt * t * t * p2 + t * t * t
    }
    // Newton-Raphson on x(t) = x, fallback to bisection.
    let mut t = x;
    for _ in 0..8 {
        let tx = bezier_component(t, x1, x2) - x;
        if tx.abs() < 1e-6 {
            break;
        }
        let d = 3.0 * (1.0 - t) * (1.0 - t) * x1
            + 6.0 * (1.0 - t) * t * (x2 - x1)
            + 3.0 * t * t * (1.0 - x2);
        if d.abs() < 1e-6 {
            break;
        }
        t -= tx / d;
        t = t.clamp(0.0, 1.0);
    }
    if (bezier_component(t, x1, x2) - x).abs() > 1e-4 {
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            let mid = (lo + hi) * 0.5;
            if bezier_component(mid, x1, x2) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        t = (lo + hi) * 0.5;
    }
    bezier_component(t, y1, y2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_track(values: &[(u64, f32)]) -> KeyframeTrack {
        let mut track = KeyframeTrack::new("transform.position.x", InterpolationMethod::Linear);
        for &(frame, value) in values {
            track.upsert_keyframe(Keyframe::at(frame, value));
        }
        track
    }

    #[test]
    fn boundary_clamping() {
        let track = linear_track(&[(10, 5.0), (20, 15.0)]);
        assert!(track.evaluate(0) == 5.0);
        assert!(track.evaluate(10) == 5.0);
        assert!(track.evaluate(25) == 15.0);
        assert!(track.evaluate(20) == 15.0);
    }

    #[test]
    fn empty_and_single_keyframes() {
        let empty = KeyframeTrack::new("p", InterpolationMethod::Linear);
        assert!(empty.evaluate(7) == 0.0);
        let single = linear_track(&[(5, 3.5)]);
        assert!(single.evaluate(100) == 3.5);
    }

    #[test]
    fn linear_midpoint() {
        let track = linear_track(&[(0, 0.0), (10, 10.0)]);
        assert!((track.evaluate(5) - 5.0).abs() < 1e-6);
        assert!((track.evaluate(2) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn upsert_keeps_sorted_and_replaces() {
        let mut track = linear_track(&[(10, 1.0), (20, 2.0)]);
        track.upsert_keyframe(Keyframe::at(15, 9.0));
        assert_eq!(track.keyframes.len(), 3);
        assert_eq!(track.keyframes[1].frame, 15);
        track.upsert_keyframe(Keyframe::at(15, 7.0));
        assert_eq!(track.keyframes.len(), 3);
        assert_eq!(track.keyframes[1].value, 7.0);
    }

    #[test]
    fn cubic_smooths_through_midpoint() {
        let mut track = KeyframeTrack::new("v", InterpolationMethod::Cubic);
        track.upsert_keyframe(Keyframe::at(0, 0.0));
        track.upsert_keyframe(Keyframe::at(100, 100.0));
        // Monotone track: cubic must stay between endpoints and hit the exact
        // keyframe values.
        let v = track.evaluate(50);
        assert!((50.0..=100.0).contains(&v), "v = {v}");
        assert_eq!(track.evaluate(0), 0.0);
        assert_eq!(track.evaluate(100), 100.0);
    }

    #[test]
    fn bezier_ease_matches_published_css_values() {
        // The CSS "ease" curve is (0.25, 0.1, 0.25, 1.0). Published reference
        // samples: ease(0.1) ≈ 0.0937, ease(0.5) ≈ 0.8024.
        let mut track = KeyframeTrack::new("v", InterpolationMethod::Bezier);
        track.upsert_keyframe(Keyframe::bezier(0, 0.0, 0.25, 0.1, 0.25, 1.0));
        track.upsert_keyframe(Keyframe::at(100, 1.0));
        let early = track.evaluate(10);
        assert!((early - 0.0937).abs() < 5e-3, "early = {early}");
        let mid = track.evaluate(50);
        assert!((mid - 0.8024).abs() < 5e-3, "mid = {mid}");
        // Endpoints exact.
        assert_eq!(track.evaluate(0), 0.0);
        assert_eq!(track.evaluate(100), 1.0);
    }

    #[test]
    fn bezier_extremes_reachable() {
        // Segment easing comes from the LEFT keyframe. A hard ease-in curve
        // stays nearly flat, then shoots up.
        let mut track = KeyframeTrack::new("v", InterpolationMethod::Bezier);
        track.upsert_keyframe(Keyframe::bezier(0, 0.0, 0.7, 0.0, 1.0, 0.5));
        track.upsert_keyframe(Keyframe::at(100, 1.0));
        assert!(track.evaluate(20) < 0.02, "slow start");
        assert!((track.evaluate(100) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sample_range_bakes() {
        let track = linear_track(&[(0, 0.0), (10, 10.0)]);
        let baked = track.sample_range(0, 10);
        assert_eq!(baked.len(), 11);
        assert_eq!(baked[5], 5.0);
    }
}
