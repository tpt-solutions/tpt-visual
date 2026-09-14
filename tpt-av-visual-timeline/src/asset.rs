//! Video asset definitions.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::AssetId;
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

/// A reference to a video file on disk.
///
/// Assets are immutable metadata; the media itself is never modified by the
/// engine (strict non-destructive tenet).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoAsset {
    /// Unique asset identifier.
    pub id: AssetId,
    /// File path to the source video file.
    pub file_path: PathBuf,
    /// Duration of the asset, in frames at `frame_rate`.
    pub duration_frames: u64,
    /// Frame rate of the asset.
    pub frame_rate: FrameRate,
    /// Resolution of the asset.
    pub resolution: Resolution,
    /// Pixel format of the decoded frames (e.g. YUV420).
    pub pixel_format: PixelFormat,
    /// Color space of the asset (e.g. Rec.709, Rec.2020).
    pub color_space: String,
}

impl VideoAsset {
    /// Creates a new asset with a fresh id.
    #[must_use]
    pub fn new(
        id: AssetId,
        file_path: impl Into<PathBuf>,
        duration_frames: u64,
        frame_rate: FrameRate,
        resolution: Resolution,
        pixel_format: PixelFormat,
        color_space: impl Into<String>,
    ) -> Self {
        Self {
            id,
            file_path: file_path.into(),
            duration_frames,
            frame_rate,
            resolution,
            pixel_format,
            color_space: color_space.into(),
        }
    }

    /// The asset file name (for UI display).
    #[must_use]
    pub fn name(&self) -> String {
        Path::file_name(&self.file_path)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Duration in seconds.
    #[must_use]
    pub fn duration_secs(&self) -> f64 {
        self.frame_rate.time_of_frame(self.duration_frames)
    }
}
