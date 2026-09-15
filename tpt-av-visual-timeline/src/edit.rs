//! Edit operations: insert, delete, move, and split clips.
//!
//! Each operation is a [`Reversible`] command: it knows how to apply itself
//! and how to undo itself, so a [`History`](crate::History) can drive
//! undo/redo without cloning whole sessions.

use crate::history::Reversible;
use crate::session::Session;
use crate::{Clip, ClipId, Result, TimelineError, TrackId};

/// Validates that `clip` fits on `track` (no overlap) without mutating.
fn check_insert(session: &Session, track_id: TrackId, clip: &Clip) -> Result<()> {
    let track = session.track_checked(track_id)?;
    if let Some(other) = track.clips.iter().find(|c| c.overlaps(clip)) {
        return Err(TimelineError::Invalid(format!(
            "clip {} overlaps clip {} on track {track_id}",
            clip.id, other.id
        )));
    }
    Ok(())
}

/// Inserts a clip onto a track.
#[derive(Debug, Clone)]
pub struct InsertClip {
    pub(crate) track_id: TrackId,
    pub(crate) clip: Clip,
}

impl InsertClip {
    /// Creates the operation; use [`Session::allocate_clip_id`] to give the
    /// clip a fresh id first.
    pub fn new(track_id: TrackId, clip: Clip) -> Self {
        InsertClip { track_id, clip }
    }
}

impl Reversible for InsertClip {
    fn apply(&self, session: &mut Session) -> Result<()> {
        check_insert(session, self.track_id, &self.clip)?;
        session
            .track_checked_mut(self.track_id)?
            .insert_clip(self.clip.clone())
    }

    fn revert(&self, session: &mut Session) -> Result<()> {
        session
            .track_checked_mut(self.track_id)?
            .remove_clip(self.clip.id)?;
        Ok(())
    }

    fn describe(&self) -> String {
        format!("insert clip {} on track {}", self.clip.id, self.track_id)
    }
}

/// Deletes a clip from its track.
///
/// Created eagerly via [`DeleteClip::perform`], which captures the removed
/// clip so undo can restore it verbatim.
#[derive(Debug, Clone)]
pub struct DeleteClip {
    pub(crate) track_id: TrackId,
    pub(crate) clip: Clip,
}

impl DeleteClip {
    /// Removes the clip immediately, returning the reversible operation.
    pub fn perform(session: &mut Session, track_id: TrackId, clip_id: ClipId) -> Result<Self> {
        let clip = session.track_checked_mut(track_id)?.remove_clip(clip_id)?;
        Ok(DeleteClip { track_id, clip })
    }
}

impl Reversible for DeleteClip {
    /// Redo path: the delete itself (`perform` already applied it once).
    fn apply(&self, session: &mut Session) -> Result<()> {
        session
            .track_checked_mut(self.track_id)?
            .remove_clip(self.clip.id)?;
        Ok(())
    }

    /// Undo path: restore the deleted clip at its original position.
    fn revert(&self, session: &mut Session) -> Result<()> {
        check_insert(session, self.track_id, &self.clip)?;
        session
            .track_checked_mut(self.track_id)?
            .insert_clip(self.clip.clone())
    }

    fn describe(&self) -> String {
        format!("delete clip {}", self.clip.id)
    }
}

/// Moves a clip to a new track and/or start frame.
#[derive(Debug, Clone)]
pub struct MoveClip {
    pub(crate) clip_id: ClipId,
    pub(crate) new_track_id: TrackId,
    pub(crate) new_start_frame: u64,
    pub(crate) from_track_id: TrackId,
    pub(crate) from_start_frame: u64,
}

impl MoveClip {
    /// Creates the operation from a resolved clip location.
    pub fn new(
        clip_id: ClipId,
        from_track_id: TrackId,
        from_start_frame: u64,
        new_track_id: TrackId,
        new_start_frame: u64,
    ) -> Self {
        MoveClip {
            clip_id,
            new_track_id,
            new_start_frame,
            from_track_id,
            from_start_frame,
        }
    }
}

impl Reversible for MoveClip {
    fn apply(&self, session: &mut Session) -> Result<()> {
        // Validate everything before mutating so a failed move leaves the
        // session untouched.
        let clip = session
            .track_checked(self.from_track_id)?
            .clip(self.clip_id)
            .ok_or_else(|| TimelineError::NotFound(format!("clip {}", self.clip_id)))?
            .clone();
        let mut moved = clip.clone();
        moved.start_frame = self.new_start_frame;
        {
            let dest = session.track_checked(self.new_track_id)?;
            if let Some(other) = dest
                .clips
                .iter()
                .find(|c| c.id != self.clip_id && c.overlaps(&moved))
            {
                return Err(TimelineError::Invalid(format!(
                    "clip {} overlaps clip {} on track {}",
                    moved.id, other.id, self.new_track_id
                )));
            }
        }
        session
            .track_checked_mut(self.from_track_id)?
            .remove_clip(self.clip_id)?;
        session
            .track_checked_mut(self.new_track_id)?
            .insert_clip(moved)
    }

    fn revert(&self, session: &mut Session) -> Result<()> {
        // Remove from the destination, restore at the original position —
        // validated up front so a failed undo does not tear state.
        let mut original = session
            .track_checked(self.new_track_id)?
            .clip(self.clip_id)
            .ok_or_else(|| TimelineError::NotFound(format!("clip {}", self.clip_id)))?
            .clone();
        original.start_frame = self.from_start_frame;
        {
            let src = session.track_checked(self.from_track_id)?;
            if let Some(other) = src
                .clips
                .iter()
                .find(|c| c.id != self.clip_id && c.overlaps(&original))
            {
                return Err(TimelineError::Invalid(format!(
                    "clip {} overlaps clip {} on track {}",
                    original.id, other.id, self.from_track_id
                )));
            }
        }
        session
            .track_checked_mut(self.new_track_id)?
            .remove_clip(self.clip_id)?;
        session
            .track_checked_mut(self.from_track_id)?
            .insert_clip(original)
    }

    fn describe(&self) -> String {
        format!("move clip {}", self.clip_id)
    }
}

/// Splits a clip at a timeline frame.
#[derive(Debug, Clone)]
pub struct SplitClip {
    pub(crate) left_id: ClipId,
    pub(crate) right_id: ClipId,
    pub(crate) track_id: TrackId,
    pub(crate) at_frame: u64,
    pub(crate) original: Clip,
}

impl SplitClip {
    /// Performs the split immediately, returning the operation that can undo
    /// it. (Unlike the other ops this one runs eagerly so callers get the new
    /// clip id.)
    pub fn perform(session: &mut Session, clip_id: ClipId, at_frame: u64) -> Result<Self> {
        let (track_id, original) = session
            .locate_clip(clip_id)
            .ok_or_else(|| TimelineError::NotFound(format!("clip {clip_id}")))?;
        if session.track_checked(track_id)?.locked {
            return Err(TimelineError::Invalid(format!(
                "track {track_id} is locked"
            )));
        }
        let original = original.clone();
        let right_id = session.allocate_clip_id();
        let op = SplitClip {
            left_id: clip_id,
            right_id,
            track_id,
            at_frame,
            original,
        };
        op.apply(session)?;
        Ok(op)
    }
}

impl Reversible for SplitClip {
    fn apply(&self, session: &mut Session) -> Result<()> {
        let track = session.track_checked_mut(self.track_id)?;
        let clip = track
            .clip_mut(self.left_id)
            .ok_or_else(|| TimelineError::NotFound(format!("clip {}", self.left_id)))?;
        let right = clip.split(self.right_id, self.at_frame)?;
        track.insert_clip(right)
    }

    fn revert(&self, session: &mut Session) -> Result<()> {
        let track = session.track_checked_mut(self.track_id)?;
        track.remove_clip(self.right_id)?;
        let left = track
            .clip_mut(self.left_id)
            .ok_or_else(|| TimelineError::NotFound(format!("clip {}", self.left_id)))?;
        *left = self.original.clone();
        Ok(())
    }

    fn describe(&self) -> String {
        format!("split clip {} at frame {}", self.left_id, self.at_frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AssetId;
    use tpt_av_visual_utils::FrameRate;

    fn session() -> Session {
        let mut s = Session::new(
            "test",
            FrameRate::film(),
            tpt_av_visual_utils::Resolution::full_hd(),
        );
        s.add_track("Video 2");
        s
    }

    fn clip(session: &mut Session, start: u64, duration: u64) -> Clip {
        if session.assets.is_empty() {
            session.register_asset(crate::VideoAsset::new(
                AssetId(0),
                "a.mp4",
                1000,
                FrameRate::film(),
                tpt_av_visual_utils::Resolution::full_hd(),
                tpt_av_visual_utils::PixelFormat::Yuv420p,
                "Rec709",
            ));
        }
        let asset_id = *session.assets.keys().next().unwrap();
        Clip::new(session.allocate_clip_id(), asset_id, start, 0, duration)
    }

    #[test]
    fn insert_and_undo() {
        let mut s = session();
        let track0 = s.tracks[0].id;
        let c = clip(&mut s, 0, 50);
        let op = InsertClip::new(track0, c.clone());
        op.apply(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 1);
        op.revert(&mut s).unwrap();
        assert!(s.tracks[0].clips.is_empty());
    }

    #[test]
    fn insert_rejects_overlap_without_mutating() {
        let mut s = session();
        let track0 = s.tracks[0].id;
        let c1 = clip(&mut s, 0, 50);
        s.track_checked_mut(track0)
            .unwrap()
            .insert_clip(c1)
            .unwrap();
        let c2 = clip(&mut s, 25, 10);
        let op = InsertClip::new(track0, c2);
        assert!(op.apply(&mut s).is_err());
    }

    #[test]
    fn delete_and_undo_restores() {
        let mut s = session();
        let track0 = s.tracks[0].id;
        let c = clip(&mut s, 10, 50);
        s.track_checked_mut(track0)
            .unwrap()
            .insert_clip(c.clone())
            .unwrap();
        let op = DeleteClip::perform(&mut s, track0, c.id).unwrap();
        assert!(s.tracks[0].clips.is_empty());
        op.revert(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips[0], c);
        // Re-delete via apply (redo path).
        op.apply(&mut s).unwrap();
        assert!(s.tracks[0].clips.is_empty());
    }

    #[test]
    fn move_within_and_across_tracks() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let t1 = s.tracks[1].id;
        let c = clip(&mut s, 0, 50);
        s.track_checked_mut(t0)
            .unwrap()
            .insert_clip(c.clone())
            .unwrap();

        let op = MoveClip::new(c.id, t0, 0, t0, 100);
        op.apply(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips[0].start_frame, 100);
        op.revert(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips[0].start_frame, 0);

        let op = MoveClip::new(c.id, t0, 0, t1, 40);
        op.apply(&mut s).unwrap();
        assert!(s.tracks[0].clips.is_empty());
        assert_eq!(s.tracks[1].clips[0].start_frame, 40);
        op.revert(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips[0].start_frame, 0);
        assert!(s.tracks[1].clips.is_empty());
    }

    #[test]
    fn move_rejected_when_destination_overlaps() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let a = clip(&mut s, 0, 50);
        let b = clip(&mut s, 100, 50);
        s.track_checked_mut(t0)
            .unwrap()
            .insert_clip(a.clone())
            .unwrap();
        s.track_checked_mut(t0).unwrap().insert_clip(b).unwrap();
        let op = MoveClip::new(a.id, t0, 0, t0, 120);
        assert!(op.apply(&mut s).is_err());
        // State unchanged after failed move.
        assert_eq!(s.tracks[0].clips[0].start_frame, 0);
    }

    #[test]
    fn split_perform_and_history_undo() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let c = clip(&mut s, 100, 60);
        s.track_checked_mut(t0)
            .unwrap()
            .insert_clip(c.clone())
            .unwrap();

        let op = SplitClip::perform(&mut s, c.id, 130).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 2);
        let (left, right) = (&s.tracks[0].clips[0], &s.tracks[0].clips[1]);
        assert_eq!(left.end_frame(), 130);
        assert_eq!(right.start_frame, 130);
        assert_eq!(right.duration_frames, 30);
        assert_eq!(right.source_offset, 30);

        op.revert(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 1);
        assert_eq!(s.tracks[0].clips[0], c);

        // Re-apply works too (redo path).
        op.apply(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 2);
    }

    #[test]
    fn split_outside_span_rejected() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let c = clip(&mut s, 100, 10);
        s.track_checked_mut(t0).unwrap().insert_clip(c).unwrap();
        assert!(SplitClip::perform(&mut s, ClipId(999), 105).is_err());
    }
}
