//! `tpt-av-visual-timeline` — the pure data model of a non-destructive video
//! edit.
//!
//! This crate does not process video. It only describes *what* video should be
//! displayed, *when*, and *how*: a [`session::Session`] holds ordered
//! [`track::Track`]s, each track holds [`clip::Clip`]s that reference
//! [`asset::VideoAsset`]s by id, and every clip carries a
//! [`transform::Transform`], opacity, blend mode, keyframe animations, and
//! effect parameters. All edits are non-destructive: original media files are
//! never mutated, and every change goes through [`edit`] operations recorded
//! in [`history::History`] for undo/redo.
//!
//! # Ecosystem
//!
//! `tpt-kinetix` decodes video files into raw frames; this crate is the edit
//! state that decides which of those frames the
//! `tpt-av-visual-compositor` renders.

pub mod asset;
pub mod clip;
pub mod edit;
pub mod history;
pub mod keyframe;
pub mod session;
pub mod track;
pub mod transform;

pub use asset::VideoAsset;
pub use clip::{BlendMode, Clip, EffectInstance};
pub use history::History;
pub use keyframe::{InterpolationMethod, Keyframe, KeyframeTrack};
pub use session::{Session, SessionMetadata};
pub use track::Track;
pub use transform::Transform;

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u64);

        impl $name {
            /// The raw numeric id.
            #[must_use]
            pub const fn value(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}

define_id!(
    /// Unique identifier of a [`Session`](session::Session).
    SessionId
);
define_id!(
    /// Unique identifier of a [`VideoAsset`].
    AssetId
);
define_id!(
    /// Unique identifier of a [`Track`].
    TrackId
);
define_id!(
    /// Unique identifier of a [`Clip`].
    ClipId
);

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, TimelineError>;

/// Errors produced by timeline operations.
#[derive(Debug, thiserror::Error)]
pub enum TimelineError {
    /// The referenced entity does not exist.
    #[error("{0} not found")]
    NotFound(String),

    /// The operation would produce an invalid timeline (e.g. overlapping
    /// clips on one track).
    #[error("{0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_display_and_round_trip() {
        let id = ClipId(42);
        assert_eq!(id.to_string(), "ClipId(42)");
        assert_eq!(id.value(), 42);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "42");
        assert_eq!(serde_json::from_str::<ClipId>(&json).unwrap(), id);
    }
}
