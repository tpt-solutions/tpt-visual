//! Video track definitions.

use serde::{Deserialize, Serialize};

use crate::clip::BlendMode;
use crate::{Clip, ClipId, Result, TimelineError, TrackId};

/// A single video track containing multiple clips, kept sorted by start time.
///
/// Clips on one track never overlap; the edit operations enforce this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Unique track identifier.
    pub id: TrackId,
    /// Track name (e.g. "Video 1", "Title Graphics").
    pub name: String,
    /// All clips on this track, sorted by `start_frame`.
    pub clips: Vec<Clip>,
    /// Track-level opacity multiplier (0.0 = transparent, 1.0 = opaque).
    pub opacity: f32,
    /// Track-level blend mode.
    pub blend_mode: BlendMode,
    /// Whether the track is hidden (skipped entirely when rendering).
    pub hidden: bool,
    /// Whether the track is locked (edits are rejected).
    pub locked: bool,
}

impl Track {
    /// Creates an empty track.
    #[must_use]
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Track {
            id,
            name: name.into(),
            clips: Vec::new(),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            hidden: false,
            locked: false,
        }
    }

    /// Whether the track contributes to the rendered output.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !self.hidden && self.opacity > 0.0
    }

    /// Finds a clip by id.
    #[must_use]
    pub fn clip(&self, clip_id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == clip_id)
    }

    /// Mutable variant of [`Track::clip`].
    pub fn clip_mut(&mut self, clip_id: ClipId) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == clip_id)
    }

    /// The clip covering `frame`, if any.
    #[must_use]
    pub fn clip_at(&self, frame: u64) -> Option<&Clip> {
        self.clips.iter().find(|c| c.contains_frame(frame))
    }

    /// Inserts a clip, keeping the list sorted and rejecting overlaps with
    /// clips already on the track.
    pub fn insert_clip(&mut self, clip: Clip) -> Result<()> {
        if let Some(other) = self.clips.iter().find(|c| c.overlaps(&clip)) {
            return Err(TimelineError::Invalid(format!(
                "clip {} overlaps clip {} on track {}",
                clip.id, other.id, self.id
            )));
        }
        let pos = self
            .clips
            .partition_point(|c| c.start_frame <= clip.start_frame);
        self.clips.insert(pos, clip);
        Ok(())
    }

    /// Removes a clip by id, returning it.
    pub fn remove_clip(&mut self, clip_id: ClipId) -> Result<Clip> {
        let pos = self
            .clips
            .iter()
            .position(|c| c.id == clip_id)
            .ok_or_else(|| TimelineError::NotFound(format!("clip {clip_id}")))?;
        Ok(self.clips.remove(pos))
    }

    /// The last frame covered by any clip on this track (exclusive).
    #[must_use]
    pub fn end_frame(&self) -> u64 {
        self.clips
            .last()
            .map_or(0, |c| c.end_frame())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AssetId;

    fn track() -> Track {
        Track::new(TrackId(1), "Video 1")
    }

    fn clip(id: u64, start: u64, duration: u64) -> Clip {
        Clip::new(ClipId(id), AssetId(9), start, 0, duration)
    }

    #[test]
    fn insert_keeps_sorted() {
        let mut t = track();
        t.insert_clip(clip(1, 100, 10)).unwrap();
        t.insert_clip(clip(2, 0, 50)).unwrap();
        t.insert_clip(clip(3, 50, 50)).unwrap();
        let starts: Vec<u64> = t.clips.iter().map(|c| c.start_frame).collect();
        assert_eq!(starts, vec![0, 50, 100]);
        assert_eq!(t.end_frame(), 110);
    }

    #[test]
    fn insert_rejects_overlap() {
        let mut t = track();
        t.insert_clip(clip(1, 0, 50)).unwrap();
        let err = t.insert_clip(clip(2, 49, 2)).unwrap_err();
        assert!(err.to_string().contains("overlaps"));
        // Touching edges are fine.
        t.insert_clip(clip(2, 50, 50)).unwrap();
    }

    #[test]
    fn lookup_helpers() {
        let mut t = track();
        t.insert_clip(clip(1, 10, 10)).unwrap();
        assert_eq!(t.clip_at(15).unwrap().id, ClipId(1));
        assert!(t.clip_at(9).is_none());
        assert!(t.clip_at(20).is_none());
        assert_eq!(t.clip(ClipId(1)).unwrap().id, ClipId(1));
        assert!(t.clip(ClipId(99)).is_none());
        t.clip_mut(ClipId(1)).unwrap().opacity = 0.25;
        assert_eq!(t.clip(ClipId(1)).unwrap().opacity, 0.25);
    }

    #[test]
    fn remove_returns_clip() {
        let mut t = track();
        t.insert_clip(clip(1, 0, 10)).unwrap();
        let removed = t.remove_clip(ClipId(1)).unwrap();
        assert_eq!(removed.id, ClipId(1));
        assert!(t.remove_clip(ClipId(1)).is_err());
    }

    #[test]
    fn visibility() {
        let mut t = track();
        assert!(t.is_visible());
        t.hidden = true;
        assert!(!t.is_visible());
        t.hidden = false;
        t.opacity = 0.0;
        assert!(!t.is_visible());
    }
}
