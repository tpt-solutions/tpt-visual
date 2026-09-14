//! Clip definitions — the atomic unit of a non-destructive edit.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::keyframe::KeyframeTrack;
use crate::transform::Transform;
use crate::{AssetId, ClipId, Result, TimelineError};

/// How a clip's pixels are composited over what is behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    /// Source-over alpha compositing.
    #[default]
    Normal,
    /// Darkening blend: `dst * src`.
    Multiply,
    /// Lightening blend: `1 - (1 - dst)(1 - src)`.
    Screen,
    /// Combines multiply for dark areas and screen for light areas.
    Overlay,
    /// Keeps the darker of the two.
    Darken,
    /// Keeps the lighter of the two.
    Lighten,
    /// Hard light: overlay with the roles swapped.
    HardLight,
    /// Absolute difference.
    Difference,
    /// Inverts based on backdrop.
    Exclusion,
}

impl BlendMode {
    /// Every blend mode, in shader dispatch order.
    pub const ALL: [BlendMode; 9] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::HardLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
    ];

    /// Index used by the GPU blend shader's mode uniform.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        match self {
            BlendMode::Normal => 0,
            BlendMode::Multiply => 1,
            BlendMode::Screen => 2,
            BlendMode::Overlay => 3,
            BlendMode::Darken => 4,
            BlendMode::Lighten => 5,
            BlendMode::HardLight => 6,
            BlendMode::Difference => 7,
            BlendMode::Exclusion => 8,
        }
    }
}

/// A named effect and its parameters, attached to a clip.
///
/// The timeline is a pure data model: it stores the *name* and a simple
/// numeric parameter bag; the effects crate resolves the name to a concrete
/// implementation. Property paths like `"effects.0.brightness"` can be
/// animated via keyframes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    /// Registered effect name (e.g. `"gaussian_blur"`).
    pub effect_name: String,
    /// Numeric parameters (e.g. `"radius" → 4.0`).
    pub parameters: BTreeMap<String, f32>,
}

impl EffectInstance {
    /// Creates an effect instance with no parameters.
    #[must_use]
    pub fn new(effect_name: impl Into<String>) -> Self {
        EffectInstance {
            effect_name: effect_name.into(),
            parameters: BTreeMap::new(),
        }
    }

    /// Builder-style parameter setter.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: f32) -> Self {
        self.parameters.insert(key.into(), value);
        self
    }

    /// Reads a parameter with a fallback default.
    #[must_use]
    pub fn param(&self, key: &str, default: f32) -> f32 {
        self.parameters.get(key).copied().unwrap_or(default)
    }
}

/// A non-destructive reference to a video asset placed on the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Unique clip identifier.
    pub id: ClipId,
    /// Reference to the source video asset.
    pub asset_id: AssetId,
    /// Start time of the clip on the timeline, in session frames.
    pub start_frame: u64,
    /// Offset into the source asset, in source frames (trims the head).
    pub source_offset: u64,
    /// Duration of the clip, in session frames.
    pub duration_frames: u64,
    /// Spatial transform (position, scale, rotation).
    pub transform: Transform,
    /// Opacity (0.0–1.0).
    pub opacity: f32,
    /// Blend mode.
    pub blend_mode: BlendMode,
    /// Keyframe animations.
    pub keyframes: Vec<KeyframeTrack>,
    /// Applied effects.
    pub effects: Vec<EffectInstance>,
}

impl Clip {
    /// Creates a basic clip covering `[start, start + duration)`.
    #[must_use]
    pub fn new(
        id: ClipId,
        asset_id: AssetId,
        start_frame: u64,
        source_offset: u64,
        duration_frames: u64,
    ) -> Self {
        Clip {
            id,
            asset_id,
            start_frame,
            source_offset,
            duration_frames,
            transform: Transform::default(),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            keyframes: Vec::new(),
            effects: Vec::new(),
        }
    }

    /// Exclusive end frame on the timeline (`start + duration`).
    #[must_use]
    pub fn end_frame(&self) -> u64 {
        self.start_frame + self.duration_frames
    }

    /// Whether the clip covers the given timeline frame.
    #[must_use]
    pub fn contains_frame(&self, frame: u64) -> bool {
        frame >= self.start_frame && frame < self.end_frame()
    }

    /// Maps a timeline frame to the source frame it plays.
    #[must_use]
    pub fn source_frame_at(&self, timeline_frame: u64) -> u64 {
        self.source_offset + (timeline_frame - self.start_frame)
    }

    /// Whether the clips overlap on the timeline.
    #[must_use]
    pub fn overlaps(&self, other: &Clip) -> bool {
        self.start_frame < other.end_frame() && other.start_frame < self.end_frame()
    }

    /// Splits the clip at `at_frame` (a timeline frame strictly inside the
    /// clip), returning the *right-hand* part. `self` is shortened to the
    /// left part. The right part gets `new_id`.
    ///
    /// The trim on the source advances by the same amount, so the right part
    /// continues playing exactly where the original would have.
    pub fn split(&mut self, new_id: ClipId, at_frame: u64) -> Result<Clip> {
        if at_frame <= self.start_frame || at_frame >= self.end_frame() {
            return Err(TimelineError::Invalid(format!(
                "split frame {at_frame} outside clip span [{}, {})",
                self.start_frame,
                self.end_frame()
            )));
        }
        let left_len = at_frame - self.start_frame;
        let mut right = self.clone();
        right.id = new_id;
        right.start_frame = at_frame;
        right.source_offset = self.source_offset + left_len;
        right.duration_frames = self.duration_frames - left_len;
        self.duration_frames = left_len;
        Ok(right)
    }

    /// Reads an animated property value at `frame`, falling back to the
    /// clip's static value for built-in properties.
    ///
    /// Recognized built-ins: `transform.position.x/y`, `transform.scale.x/y`,
    /// `transform.rotation`, `opacity`. Any other property path returns the
    /// matching keyframe track's value (or `default` when absent).
    #[must_use]
    pub fn property_value(&self, property: &str, frame: u64) -> f32 {
        if let Some(track) = self.keyframes.iter().find(|k| k.property == property) {
            return track.evaluate(frame);
        }
        match property {
            "transform.position.x" => self.transform.position.0,
            "transform.position.y" => self.transform.position.1,
            "transform.scale.x" => self.transform.scale.0,
            "transform.scale.y" => self.transform.scale.1,
            "transform.rotation" => self.transform.rotation,
            "opacity" => self.opacity,
            _ => 0.0,
        }
    }

    /// Applies keyframed property values at `frame`, returning an effective
    /// transform/opacity snapshot for rendering.
    #[must_use]
    pub fn effective_state_at(&self, frame: u64) -> (Transform, f32) {
        let mut transform = self.transform;
        transform.position.0 = self.property_value("transform.position.x", frame);
        transform.position.1 = self.property_value("transform.position.y", frame);
        transform.scale.0 = self.property_value("transform.scale.x", frame);
        transform.scale.1 = self.property_value("transform.scale.y", frame);
        transform.rotation = self.property_value("transform.rotation", frame);
        let opacity = self.property_value("opacity", frame).clamp(0.0, 1.0);
        (transform, opacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyframe::{InterpolationMethod, Keyframe, KeyframeTrack};

    fn clip(start: u64, duration: u64) -> Clip {
        Clip::new(ClipId(1), AssetId(2), start, 0, duration)
    }

    #[test]
    fn span_helpers() {
        let c = clip(100, 50);
        assert_eq!(c.end_frame(), 150);
        assert!(c.contains_frame(100));
        assert!(c.contains_frame(149));
        assert!(!c.contains_frame(150));
        assert_eq!(c.source_frame_at(110), 10);
    }

    #[test]
    fn overlap_detection() {
        let a = clip(0, 50);
        let b = clip(49, 10);
        let c = clip(50, 10);
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn split_at_arbitrary_offset() {
        let mut left = Clip::new(ClipId(1), AssetId(7), 100, 25, 60);
        left.opacity = 0.5;
        left.blend_mode = BlendMode::Screen;
        left.effects.push(EffectInstance::new("gaussian_blur").with_param("radius", 3.0));

        let right = left.split(ClipId(2), 130).expect("split inside clip");
        assert_eq!(left.start_frame, 100);
        assert_eq!(left.duration_frames, 30);
        assert_eq!(left.end_frame(), 130);

        assert_eq!(right.id, ClipId(2));
        assert_eq!(right.start_frame, 130);
        assert_eq!(right.duration_frames, 30);
        assert_eq!(right.source_offset, 55); // 25 + 30
        assert_eq!(right.asset_id, AssetId(7));
        assert_eq!(right.opacity, 0.5, "attributes carry over");
        assert_eq!(right.blend_mode, BlendMode::Screen);
        assert_eq!(right.effects, left.effects);

        // Seamless playback across the cut.
        assert_eq!(left.source_frame_at(129), 25 + 29);
        assert_eq!(right.source_frame_at(130), 55);
    }

    #[test]
    fn split_outside_span_rejected() {
        let mut c = clip(10, 10);
        assert!(c.split(ClipId(2), 10).is_err()); // at start
        assert!(c.split(ClipId(2), 20).is_err()); // at end
        assert!(c.split(ClipId(2), 50).is_err()); // beyond
        assert_eq!(c.duration_frames, 10, "failed split must not mutate");
    }

    #[test]
    fn keyframed_property_values() {
        let mut c = clip(0, 100);
        c.opacity = 0.8;
        let mut track =
            KeyframeTrack::new("transform.position.y", InterpolationMethod::Linear);
        track.upsert_keyframe(Keyframe::at(0, 0.0));
        track.upsert_keyframe(Keyframe::at(100, 200.0));
        c.keyframes.push(track);

        assert!((c.property_value("transform.position.y", 50) - 100.0).abs() < 1e-4);
        assert_eq!(c.property_value("opacity", 40), 0.8); // static fallback
        let (t, o) = c.effective_state_at(25);
        assert!((t.position.1 - 50.0).abs() < 1e-4);
        assert_eq!(o, 0.8);
    }

    #[test]
    fn effect_param_helpers() {
        let e = EffectInstance::new("chroma_key")
            .with_param("tolerance", 0.4)
            .with_param("softness", 0.1);
        assert_eq!(e.param("tolerance", 0.0), 0.4);
        assert_eq!(e.param("missing", 0.9), 0.9);
    }
}
