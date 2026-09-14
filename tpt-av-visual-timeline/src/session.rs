//! The top-level session: a complete video editing project.

use serde::{Deserialize, Serialize};

use crate::asset::VideoAsset;
use crate::clip::Clip;
use crate::track::Track;
use crate::{AssetId, ClipId, Result, SessionId, TimelineError, TrackId};
use tpt_av_visual_utils::{FrameRate, Resolution};
use std::collections::BTreeMap;

/// Global session metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    /// Timecode of the first frame (in frames from zero, for display).
    pub timecode_start_frame: u64,
    /// Working color space of the session (e.g. "Rec709").
    pub working_color_space: String,
    /// Free-form descriptive text.
    pub description: String,
    /// Arbitrary user tags.
    pub tags: BTreeMap<String, String>,
}

/// A complete video editing session: settings, assets, and tracks.
///
/// The session is the serializable document of a `tpt-visual` project. It is
/// pure data — rendering happens in `tpt-av-visual-compositor`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Session name (e.g. "My Documentary").
    pub name: String,
    /// Frame rate for the entire session.
    pub frame_rate: FrameRate,
    /// Resolution for the entire session.
    pub resolution: Resolution,
    /// All tracks in the session (ordered bottom-to-top by z-index).
    pub tracks: Vec<Track>,
    /// Assets referenced by clips (keyed by asset id).
    pub assets: BTreeMap<AssetId, VideoAsset>,
    /// Global metadata (timecode start, working color space, etc.).
    pub metadata: SessionMetadata,
    /// Id allocation counters (managed by the session).
    next_track_id: u64,
    next_clip_id: u64,
    next_asset_id: u64,
}

impl Session {
    /// Creates a new session with a single empty track named "Video 1".
    pub fn new(name: impl Into<String>, frame_rate: FrameRate, resolution: Resolution) -> Self {
        let mut session = Session {
            id: SessionId(1),
            name: name.into(),
            frame_rate,
            resolution,
            tracks: Vec::new(),
            assets: BTreeMap::new(),
            metadata: SessionMetadata {
                working_color_space: "Rec709".into(),
                ..SessionMetadata::default()
            },
            next_track_id: 1,
            next_clip_id: 1,
            next_asset_id: 1,
        };
        let track_id = session.allocate_track_id();
        session.tracks.push(Track::new(track_id, "Video 1"));
        session
    }

    /// Allocates a fresh track id.
    pub fn allocate_track_id(&mut self) -> TrackId {
        let id = TrackId(self.next_track_id);
        self.next_track_id += 1;
        id
    }

    /// Allocates a fresh clip id.
    pub fn allocate_clip_id(&mut self) -> ClipId {
        let id = ClipId(self.next_clip_id);
        self.next_clip_id += 1;
        id
    }

    /// Allocates a fresh asset id and registers the asset.
    pub fn register_asset(&mut self, mut asset: VideoAsset) -> VideoAsset {
        asset.id = AssetId(self.next_asset_id);
        self.next_asset_id += 1;
        self.assets.insert(asset.id, asset.clone());
        asset
    }

    /// Adds a new track on top of the stack and returns its id.
    pub fn add_track(&mut self, name: impl Into<String>) -> TrackId {
        let id = self.allocate_track_id();
        self.tracks.push(Track::new(id, name));
        id
    }

    /// Finds a track by id.
    #[must_use]
    pub fn track(&self, track_id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == track_id)
    }

    /// Mutable variant of [`Session::track`].
    pub fn track_mut(&mut self, track_id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == track_id)
    }

    /// Finds a track by id or errors.
    pub fn track_checked(&self, track_id: TrackId) -> Result<&Track> {
        self.track(track_id)
            .ok_or_else(|| TimelineError::NotFound(format!("track {track_id}")))
    }

    /// Mutable checked variant of [`Session::track_checked`]; also rejects
    /// locked tracks.
    pub fn track_checked_mut(&mut self, track_id: TrackId) -> Result<&mut Track> {
        let track = self
            .tracks
            .iter_mut()
            .find(|t| t.id == track_id)
            .ok_or_else(|| TimelineError::NotFound(format!("track {track_id}")))?;
        if track.locked {
            return Err(TimelineError::Invalid(format!(
                "track {track_id} is locked"
            )));
        }
        Ok(track)
    }

    /// Finds a clip (and its track) anywhere in the session.
    #[must_use]
    pub fn locate_clip(&self, clip_id: ClipId) -> Option<(TrackId, &Clip)> {
        for track in &self.tracks {
            if let Some(clip) = track.clip(clip_id) {
                return Some((track.id, clip));
            }
        }
        None
    }

    /// Mutable variant of [`Session::locate_clip`].
    pub fn locate_clip_mut(&mut self, clip_id: ClipId) -> Option<(TrackId, &mut Clip)> {
        for track in &mut self.tracks {
            if track.clip(clip_id).is_some() {
                let pos = track.clips.iter().position(|c| c.id == clip_id);
                if let Some(pos) = pos {
                    return Some((track.id, &mut track.clips[pos]));
                }
            }
        }
        None
    }

    /// Resolves a clip's asset or errors.
    pub fn asset_for(&self, clip: &Clip) -> Result<&VideoAsset> {
        self.assets
            .get(&clip.asset_id)
            .ok_or_else(|| TimelineError::NotFound(format!("asset {}", clip.asset_id)))
    }

    /// The session duration in frames: the largest clip end across visible
    /// tracks (0 for an empty session).
    #[must_use]
    pub fn duration_frames(&self) -> u64 {
        self.tracks
            .iter()
            .filter(|t| t.is_visible())
            .map(Track::end_frame)
            .max()
            .unwrap_or(0)
    }

    /// All clips active at `frame`, bottom-to-top track order.
    #[must_use]
    pub fn active_clips_at(&self, frame: u64) -> Vec<&Clip> {
        self.tracks
            .iter()
            .filter(|t| t.is_visible())
            .filter_map(|t| t.clip_at(frame))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Clip;
    use tpt_av_visual_utils::PixelFormat;

    fn session() -> Session {
        Session::new("test", FrameRate::film(), Resolution::full_hd())
    }

    fn asset() -> VideoAsset {
        VideoAsset::new(
            AssetId(0),
            "media/clip.mp4",
            240,
            FrameRate::film(),
            Resolution::full_hd(),
            PixelFormat::Yuv420p,
            "Rec709",
        )
    }

    #[test]
    fn new_session_has_default_track() {
        let s = session();
        assert_eq!(s.tracks.len(), 1);
        assert_eq!(s.tracks[0].name, "Video 1");
        assert_eq!(s.id, SessionId(1));
    }

    #[test]
    fn id_allocation_is_sequential_and_unique() {
        let mut s = session();
        let t1 = s.allocate_track_id();
        let t2 = s.allocate_track_id();
        assert_ne!(t1, t2);
        let c1 = s.allocate_clip_id();
        let c2 = s.allocate_clip_id();
        assert_ne!(c1, c2);
        let a1 = s.register_asset(asset());
        let a2 = s.register_asset(asset());
        assert_ne!(a1.id, a2.id);
        assert!(s.assets.contains_key(&a1.id));
    }

    #[test]
    fn duration_and_active_clips() {
        let mut s = session();
        let asset = s.register_asset(asset());
        let top = s.add_track("Video 2");
        let mut c1 = Clip::new(s.allocate_clip_id(), asset.id, 0, 0, 50);
        c1.id = s.allocate_clip_id();
        s.track_checked_mut(top).unwrap().insert_clip(c1).unwrap();
        assert_eq!(s.duration_frames(), 50);
        let active = s.active_clips_at(25);
        assert_eq!(active.len(), 1);
        assert!(s.active_clips_at(100).is_empty());
    }

    #[test]
    fn locate_clip_and_locked_tracks() {
        let mut s = session();
        let asset = s.register_asset(asset());
        let clip = Clip::new(s.allocate_clip_id(), asset.id, 0, 0, 10);
        s.tracks[0].insert_clip(clip.clone()).unwrap();
        let (track_id, _) = s.locate_clip(clip.id).unwrap();
        assert_eq!(track_id, s.tracks[0].id);

        s.tracks[0].locked = true;
        assert!(s.track_checked_mut(s.tracks[0].id).is_err());
        assert!(s.track_checked(s.tracks[0].id).is_ok());
    }

    #[test]
    fn serde_round_trip_preserves_everything() {
        let mut s = session();
        let asset = s.register_asset(asset());
        let mut clip = Clip::new(s.allocate_clip_id(), asset.id, 10, 5, 20);
        clip.opacity = 0.75;
        clip.blend_mode = crate::BlendMode::Screen;
        clip.transform.rotation = 12.5;
        clip.effects.push(crate::EffectInstance::new("vignette").with_param("amount", 0.5));
        s.tracks[0].insert_clip(clip.clone()).unwrap();

        let json = serde_json::to_string_pretty(&s).unwrap();
        let mut back: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // Id counters survive so edits after a load never collide.
        let c = back.allocate_clip_id();
        assert_ne!(c, clip.id);
    }

    #[test]
    fn hidden_tracks_do_not_count_toward_duration() {
        let mut s = session();
        let asset = s.register_asset(asset());
        let clip = Clip::new(s.allocate_clip_id(), asset.id, 0, 0, 100);
        s.tracks[0].insert_clip(clip).unwrap();
        assert_eq!(s.duration_frames(), 100);
        s.tracks[0].hidden = true;
        assert_eq!(s.duration_frames(), 0);
    }
}
