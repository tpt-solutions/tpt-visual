//! Undo/redo history for session edits.
//!
//! Every edit is a [`Reversible`] command. [`History`] executes commands,
//! records them, and walks the undo/redo stacks against a [`Session`].

use crate::edit;
use crate::session::Session;
use crate::{Clip, ClipId, Result, TrackId};

/// A reversible edit operation.
pub trait Reversible: std::fmt::Debug + Send {
    /// Applies the operation.
    fn apply(&self, session: &mut Session) -> Result<()>;
    /// Undoes the operation. Must restore the exact prior state.
    fn revert(&self, session: &mut Session) -> Result<()>;
    /// Human-readable description (for edit menus / logs).
    fn describe(&self) -> String;
}

/// Undo/redo history stack with a bounded depth (oldest entries fall off).
#[derive(Debug, Default)]
pub struct History {
    undo_stack: Vec<Box<dyn Reversible>>,
    redo_stack: Vec<Box<dyn Reversible>>,
    limit: usize,
}

impl History {
    /// A history with the given maximum number of undo steps.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit,
        }
    }

    /// Applies `op` and records it, clearing the redo stack.
    pub fn commit(&mut self, session: &mut Session, op: impl Reversible + 'static) -> Result<()> {
        op.apply(session)?;
        self.undo_stack.push(Box::new(op));
        self.redo_stack.clear();
        if self.limit > 0 {
            while self.undo_stack.len() > self.limit {
                self.undo_stack.remove(0);
            }
        }
        Ok(())
    }

    /// Undoes the most recent operation. Returns `false` when there is
    /// nothing to undo.
    pub fn undo(&mut self, session: &mut Session) -> Result<bool> {
        let Some(op) = self.undo_stack.pop() else {
            return Ok(false);
        };
        op.revert(session)?;
        self.redo_stack.push(op);
        Ok(true)
    }

    /// Redoes the most recently undone operation. Returns `false` when there
    /// is nothing to redo.
    pub fn redo(&mut self, session: &mut Session) -> Result<bool> {
        let Some(op) = self.redo_stack.pop() else {
            return Ok(false);
        };
        op.apply(session)?;
        self.undo_stack.push(op);
        Ok(true)
    }

    /// Number of available undo steps.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// Number of available redo steps.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// Description of the operation `undo` would reverse.
    #[must_use]
    pub fn undo_label(&self) -> Option<String> {
        self.undo_stack.last().map(|op| op.describe())
    }

    /// Clears all history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

/// Convenience helpers mirroring the common edit menu actions.
impl History {
    /// Inserts a clip and records it.
    pub fn insert_clip(
        &mut self,
        session: &mut Session,
        track_id: TrackId,
        clip: Clip,
    ) -> Result<()> {
        self.commit(session, edit::InsertClip::new(track_id, clip))
    }

    /// Deletes a clip and records it.
    pub fn delete_clip(
        &mut self,
        session: &mut Session,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Result<()> {
        let op = edit::DeleteClip::perform(session, track_id, clip_id)?;
        self.commit(session, op)
    }

    /// Splits a clip at `at_frame` and records it.
    pub fn split_clip(
        &mut self,
        session: &mut Session,
        clip_id: ClipId,
        at_frame: u64,
    ) -> Result<ClipId> {
        let op = edit::SplitClip::perform(session, clip_id, at_frame)?;
        let new_id = op.right_id;
        self.undo_stack.push(Box::new(op));
        self.redo_stack.clear();
        Ok(new_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Clip, Session};
    use tpt_av_visual_utils::{FrameRate, Resolution};

    fn session() -> Session {
        let mut s = Session::new("h", FrameRate::film(), Resolution::full_hd());
        s.register_asset(crate::VideoAsset::new(
            crate::AssetId(0),
            "a.mp4",
            500,
            FrameRate::film(),
            Resolution::full_hd(),
            tpt_av_visual_utils::PixelFormat::Yuv420p,
            "Rec709",
        ));
        s
    }

    fn clip(session: &mut Session, start: u64, duration: u64) -> Clip {
        let asset_id = *session.assets.keys().next().unwrap();
        Clip::new(session.allocate_clip_id(), asset_id, start, 0, duration)
    }

    #[test]
    fn commit_undo_redo_cycle() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let mut h = History::default();
        let c = clip(&mut s, 0, 50);

        h.insert_clip(&mut s, t0, c.clone()).unwrap();
        assert_eq!(h.undo_depth(), 1);
        assert_eq!(h.redo_depth(), 0);
        assert_eq!(s.tracks[0].clips.len(), 1);

        assert!(h.undo(&mut s).unwrap());
        assert!(s.tracks[0].clips.is_empty());
        assert_eq!(h.undo_depth(), 0);
        assert_eq!(h.redo_depth(), 1);

        assert!(h.redo(&mut s).unwrap());
        assert_eq!(s.tracks[0].clips.len(), 1);

        // Redo stack exhausted.
        assert!(!h.redo(&mut s).unwrap());
    }

    #[test]
    fn undo_nothing_is_false() {
        let mut s = session();
        let mut h = History::default();
        assert!(!h.undo(&mut s).unwrap());
    }

    #[test]
    fn new_commit_clears_redo() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let mut h = History::default();
        let a = clip(&mut s, 0, 10);
        let b = clip(&mut s, 20, 10);

        h.insert_clip(&mut s, t0, a).unwrap();
        h.undo(&mut s).unwrap();
        h.insert_clip(&mut s, t0, b).unwrap();
        assert_eq!(h.redo_depth(), 0, "redo cleared by new edit");
        assert_eq!(h.undo_depth(), 1);
    }

    #[test]
    fn split_via_history_returns_new_id() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let mut h = History::default();
        let c = clip(&mut s, 0, 40);
        h.insert_clip(&mut s, t0, c.clone()).unwrap();

        let right = h.split_clip(&mut s, c.id, 25).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 2);
        h.undo(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 1);
        assert_eq!(s.tracks[0].clips[0], c);
        h.redo(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 2);
        assert_eq!(s.tracks[0].clips[1].id, right);
    }

    #[test]
    fn depth_limit_drops_oldest() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let mut h = History::with_limit(2);
        for start in [0, 50, 100] {
            let c = clip(&mut s, start, 10);
            h.insert_clip(&mut s, t0, c).unwrap();
        }
        assert_eq!(h.undo_depth(), 2);
        // Two undos restore to after the FIRST commit.
        h.undo(&mut s).unwrap();
        h.undo(&mut s).unwrap();
        assert_eq!(s.tracks[0].clips.len(), 1);
        assert_eq!(s.tracks[0].clips[0].start_frame, 0);
    }

    #[test]
    fn failed_commit_does_not_record() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let mut h = History::default();
        let a = clip(&mut s, 0, 50);
        h.insert_clip(&mut s, t0, a).unwrap();
        let depth = h.undo_depth();
        // Overlapping insert fails and must not be recorded.
        let b = clip(&mut s, 25, 25);
        assert!(h.insert_clip(&mut s, t0, b).is_err());
        assert_eq!(h.undo_depth(), depth);
    }

    #[test]
    fn labels_and_clear() {
        let mut s = session();
        let t0 = s.tracks[0].id;
        let mut h = History::default();
        let c = clip(&mut s, 0, 10);
        h.insert_clip(&mut s, t0, c).unwrap();
        assert!(h.undo_label().unwrap().contains("insert"));
        h.clear();
        assert_eq!(h.undo_depth(), 0);
    }
}
