//! Lock-free state synchronization between the Main/UI thread and the
//! render thread.
//!
//! The UI thread owns the authoritative `Session` and publishes immutable
//! snapshots; the render thread reads the latest snapshot without locking
//! (`arc-swap` gives wait-free reads and a single atomic pointer publish).
//! Timeline mutations never block a frame in flight — the renderer simply
//! picks the new state up on the next frame.

use arc_swap::ArcSwap;
use std::sync::Arc;
use tpt_av_visual_timeline::Session;

/// A published, immutable timeline snapshot.
pub type SessionSnapshot = Session;

/// Handle shared between the UI thread (producer) and render thread
/// (consumer).
pub struct RenderStateHandle {
    snapshot: ArcSwap<SessionSnapshot>,
}

impl RenderStateHandle {
    /// A handle initialized with `session`.
    #[must_use]
    pub fn new(session: Session) -> Self {
        RenderStateHandle {
            snapshot: ArcSwap::from_pointee(session),
        }
    }

    /// Publishes a new snapshot from the UI thread. Lock-free for readers.
    pub fn publish(&self, session: Session) {
        self.snapshot.store(Arc::new(session));
    }

    /// Loads the latest snapshot on the render thread. Wait-free.
    #[must_use]
    pub fn load(&self) -> Arc<SessionSnapshot> {
        self.snapshot.load_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_av_visual_timeline::Session;
    use tpt_av_visual_utils::{FrameRate, Resolution};

    #[test]
    fn publish_and_load_roundtrip() {
        let session = Session::new("a", FrameRate::film(), Resolution::full_hd());
        let handle = RenderStateHandle::new(session.clone());
        assert_eq!(*handle.load(), session);

        let renamed = {
            let mut s = session.clone();
            s.name = "b".into();
            s
        };
        handle.publish(renamed.clone());
        assert_eq!(handle.load().name, "b");
    }

    #[test]
    fn concurrent_publish_and_load() {
        let handle = Arc::new(RenderStateHandle::new(Session::new(
            "initial",
            FrameRate::film(),
            Resolution::full_hd(),
        )));
        let writer = handle.clone();
        let t = std::thread::spawn(move || {
            for i in 0..100 {
                let mut s = Session::new(format!("s{i}"), FrameRate::film(), Resolution::full_hd());
                s.name = format!("s{i}");
                writer.publish(s);
            }
        });
        for _ in 0..100 {
            let _ = handle.load();
        }
        t.join().unwrap();
        assert!(handle.load().name.starts_with('s'));
    }
}
