//! Fluent session construction.
//!
//! [`SessionBuilder`] removes the id bookkeeping from the common "new
//! project with a few clips" flow: assets and tracks are referenced by
//! position (the order you added them), ids are allocated for you, and
//! [`SessionBuilder::build`] hands back a validated [`Session`].

use crate::timeline::{
    AssetId, Clip, EffectInstance, KeyframeTrack, Session, TimelineError, Transform, VideoAsset,
};
use crate::utils::{FrameRate, PixelFormat, Resolution};
use crate::{BlendMode, Result};

/// A clip to place on a track, referenced by asset position.
///
/// Construction is via [`ClipSpec::new`] plus the builder-style setters.
#[derive(Debug, Clone)]
pub struct ClipSpec {
    pub(crate) asset_index: usize,
    pub(crate) track_index: usize,
    pub(crate) start_frame: u64,
    pub(crate) source_offset: u64,
    pub(crate) duration_frames: u64,
    pub(crate) transform: Transform,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) effects: Vec<EffectInstance>,
    pub(crate) keyframes: Vec<KeyframeTrack>,
}

impl ClipSpec {
    /// A clip playing asset `asset_index` (the order it was added to the
    /// builder) on track `track_index`, starting at `start_frame`. The
    /// duration defaults to one frame; set it with [`ClipSpec::duration`].
    #[must_use]
    pub fn new(track_index: usize, asset_index: usize, start_frame: u64) -> Self {
        ClipSpec {
            asset_index,
            track_index,
            start_frame,
            source_offset: 0,
            duration_frames: 1,
            transform: Transform::default(),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            effects: Vec::new(),
            keyframes: Vec::new(),
        }
    }

    /// Clip duration in session frames.
    #[must_use]
    pub fn duration(mut self, frames: u64) -> Self {
        self.duration_frames = frames;
        self
    }

    /// Source offset (trims the head of the asset).
    #[must_use]
    pub fn source_offset(mut self, frames: u64) -> Self {
        self.source_offset = frames;
        self
    }

    /// Clip transform.
    #[must_use]
    pub fn transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Clip opacity (0.0–1.0).
    #[must_use]
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Blend mode.
    #[must_use]
    pub fn blend_mode(mut self, mode: BlendMode) -> Self {
        self.blend_mode = mode;
        self
    }

    /// Adds a registered effect (name + parameters), e.g.
    /// `("gaussian_blur", [("radius", 4.0)])`.
    #[must_use]
    pub fn effect(
        mut self,
        name: &str,
        params: impl IntoIterator<Item = (&'static str, f32)>,
    ) -> Self {
        let mut instance = EffectInstance::new(name);
        for (key, value) in params {
            instance.parameters.insert(key.to_string(), value);
        }
        self.effects.push(instance);
        self
    }

    /// Adds a keyframe animation track.
    #[must_use]
    pub fn keyframes(mut self, track: KeyframeTrack) -> Self {
        self.keyframes.push(track);
        self
    }
}

/// Fluent builder for a [`Session`].
///
/// ```
/// use tpt_av_visual::prelude::*;
///
/// let session = SessionBuilder::new("Demo", FrameRate::film(), Resolution::full_hd())
///     .add_video_asset("media/interview.mp4")   // asset 0
///     .add_track("Titles")                      // track 1 (on top of "Video 1")
///     .add_clip(ClipSpec::new(0, 0, 24).duration(96))
///     .build()
///     .expect("valid edit");
///
/// assert_eq!(session.tracks.len(), 2); // default "Video 1" + "Titles"
/// assert_eq!(session.duration_frames(), 120);
/// ```
#[derive(Debug, Clone)]
pub struct SessionBuilder {
    session: Session,
    asset_ids: Vec<AssetId>,
    pending_clips: Vec<ClipSpec>,
}

impl SessionBuilder {
    /// Starts a builder with the default "Video 1" track.
    #[must_use]
    pub fn new(name: impl Into<String>, frame_rate: FrameRate, resolution: Resolution) -> Self {
        SessionBuilder {
            session: Session::new(name, frame_rate, resolution),
            asset_ids: Vec::new(),
            pending_clips: Vec::new(),
        }
    }

    /// Adds a video asset; clips reference it by position (first call →
    /// asset 0). The file is *not* opened — assets are metadata only.
    #[must_use]
    pub fn add_video_asset(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        // Placeholder metadata; callers can override via `edit_asset`.
        let asset = VideoAsset::new(
            crate::timeline::AssetId(0),
            path.clone(),
            0,
            FrameRate::film(),
            Resolution::full_hd(),
            PixelFormat::Yuv420p,
            "Rec709",
        );
        let asset = self.session.register_asset(asset);
        // Keep the user's path ordering; register_asset assigned the id.
        self.asset_ids.push(asset.id);
        self
    }

    /// Adds a video asset with full metadata in one call — the recommended
    /// way to add real media, since decoders and the compositor read the
    /// declared resolution.
    #[must_use]
    pub fn add_video_asset_with(
        mut self,
        path: impl Into<std::path::PathBuf>,
        duration_frames: u64,
        frame_rate: FrameRate,
        resolution: Resolution,
        pixel_format: PixelFormat,
        color_space: impl Into<String>,
    ) -> Self {
        let index = self.asset_ids.len();
        self = self.add_video_asset(path);
        let id = self.asset_ids[index];
        if let Some(asset) = self.session.assets.get_mut(&id) {
            asset.duration_frames = duration_frames;
            asset.frame_rate = frame_rate;
            asset.resolution = resolution;
            asset.pixel_format = pixel_format;
            asset.color_space = color_space.into();
        }
        self
    }

    /// Overrides the metadata of a previously added asset (by position).
    #[must_use]
    pub fn edit_asset(mut self, index: usize, edit: impl FnOnce(&mut VideoAsset)) -> Self {
        if let Some(id) = self.asset_ids.get(index) {
            if let Some(asset) = self.session.assets.get_mut(id) {
                edit(asset);
            }
        }
        self
    }

    /// Adds a track on top of the stack; referenced by position (0 is the
    /// default "Video 1" track).
    #[must_use]
    pub fn add_track(mut self, name: impl Into<String>) -> Self {
        self.session.add_track(name);
        self
    }

    /// Places a clip. Clips are validated on [`SessionBuilder::build`].
    #[must_use]
    pub fn add_clip(mut self, spec: ClipSpec) -> Self {
        self.pending_clips.push(spec);
        self
    }

    /// Edits session-level settings.
    #[must_use]
    pub fn metadata(mut self, edit: impl FnOnce(&mut crate::timeline::SessionMetadata)) -> Self {
        edit(&mut self.session.metadata);
        self
    }

    /// Validates the accumulated clips and builds the session.
    ///
    /// # Errors
    /// Returns [`TimelineError`] when a clip references an unknown track or
    /// asset index, or when clips overlap on one track.
    pub fn build(mut self) -> Result<Session> {
        let pending = std::mem::take(&mut self.pending_clips);
        for spec in pending {
            let asset_id = *self.asset_ids.get(spec.asset_index).ok_or_else(|| {
                TimelineError::Invalid(format!(
                    "clip references asset index {}, but only {} assets exist",
                    spec.asset_index,
                    self.asset_ids.len()
                ))
            })?;
            let track_id = self
                .session
                .tracks
                .get(spec.track_index)
                .map(|t| t.id)
                .ok_or_else(|| {
                    TimelineError::Invalid(format!(
                        "clip references track index {}, but only {} tracks exist",
                        spec.track_index,
                        self.session.tracks.len()
                    ))
                })?;

            let clip = Clip {
                id: self.session.allocate_clip_id(),
                asset_id,
                start_frame: spec.start_frame,
                source_offset: spec.source_offset,
                duration_frames: spec.duration_frames,
                transform: spec.transform,
                opacity: spec.opacity,
                blend_mode: spec.blend_mode,
                keyframes: spec.keyframes,
                effects: spec.effects,
            };
            self.session
                .track_checked_mut(track_id)?
                .insert_clip(clip)?;
        }
        Ok(self.session)
    }
}
