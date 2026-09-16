//! Timecode, duration, and frame counting helpers.
//!
//! Timecodes are non-drop-frame by design: the visual stack counts exact
//! frames and only formats them as HH:MM:SS:FF for display.

use serde::{Deserialize, Serialize};

use crate::resolution::FrameRate;
use crate::{Result, VisualError};

/// A position or duration measured in frames at a given [`FrameRate`],
/// displayable as SMPTE-style `HH:MM:SS:FF` timecode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Timecode {
    /// Total frame count from the session start.
    pub frames: u64,
    /// Frame rate the frame count is measured at.
    pub frame_rate: FrameRate,
}

impl Timecode {
    /// Creates a timecode from a frame count.
    #[must_use]
    pub fn from_frames(frames: u64, frame_rate: FrameRate) -> Self {
        Timecode { frames, frame_rate }
    }

    /// Creates a timecode from a position in seconds.
    #[must_use]
    pub fn from_seconds(seconds: f64, frame_rate: FrameRate) -> Self {
        Timecode {
            frames: frame_rate.frames_for_duration(seconds),
            frame_rate,
        }
    }

    /// Position in seconds.
    #[must_use]
    pub fn as_seconds(self) -> f64 {
        self.frame_rate.time_of_frame(self.frames)
    }

    /// Formats as `HH:MM:SS:FF` (non-drop).
    #[must_use]
    pub fn to_string_hhmmssff(self) -> String {
        let fps = self.frame_rate.as_f64().round().max(1.0) as u64;
        let f = self.frames % fps;
        let total_secs = self.frames / fps;
        let (h, m, s) = (total_secs / 3600, (total_secs / 60) % 60, total_secs % 60);
        format!("{h:02}:{m:02}:{s:02}:{f:02}")
    }

    /// Formats as SMPTE drop-frame `HH:MM:SS;FF` for NTSC-family rates
    /// (non-integer rates such as 30000/1001). Drop-frame skips two frame
    /// *numbers* at the start of each minute except every tenth minute, so
    /// the displayed clock tracks wall-clock time. Integer rates fall back
    /// to non-drop formatting (identical to [`Timecode::to_string_hhmmssff`]
    /// but with the `;` separator).
    #[must_use]
    pub fn to_string_drop_frame(self) -> String {
        let fps_r = self.frame_rate.as_f64().round().max(1.0) as u64;
        let drop = if self.frame_rate.den == 1 {
            0
        } else {
            2 * (fps_r / 30).max(1)
        };
        if drop == 0 {
            let f = self.frames % fps_r;
            let total_secs = self.frames / fps_r;
            return format!(
                "{:02}:{:02}:{:02};{:02}",
                total_secs / 3600,
                (total_secs / 60) % 60,
                total_secs % 60,
                f
            );
        }

        // Number of dropped frame numbers accumulated before `frames`:
        // two per nominal minute boundary, minus two per ten-minute
        // boundary (minutes 0, 10, 20, ... keep all their numbers).
        let minute = 60 * fps_r;
        let ten_minutes = 600 * fps_r;
        let dropped = 2 * (self.frames / minute) - 2 * (self.frames / ten_minutes);
        // Displayed frame number (the count including the skips).
        let shown = self.frames + dropped;
        let f = shown % fps_r;
        let total_secs = shown / fps_r;
        format!(
            "{:02}:{:02}:{:02};{:02}",
            total_secs / 3600,
            (total_secs / 60) % 60,
            total_secs % 60,
            f
        )
    }

    /// Parses `HH:MM:SS:FF` or `MM:SS:FF` or `SS:FF` timecode strings.
    pub fn parse(s: &str, frame_rate: FrameRate) -> Result<Self> {
        let fps = frame_rate.as_f64().round().max(1.0) as u64;
        let parts: Vec<&str> = s.split(':').collect();
        let (h, m, sec, f) = match parts.as_slice() {
            [hh, mm, ss, ff] => (*hh, *mm, *ss, *ff),
            [mm, ss, ff] => ("0", *mm, *ss, *ff),
            [ss, ff] => ("0", "0", *ss, *ff),
            _ => {
                return Err(VisualError::InvalidOperation(format!(
                    "invalid timecode: {s}"
                )))
            }
        };
        let parse_part = |value: &str, what: &str| -> Result<u64> {
            value.parse().map_err(|_| {
                VisualError::InvalidOperation(format!("invalid timecode {what}: {value}"))
            })
        };
        let hh = parse_part(h, "hours")?;
        let mm = parse_part(m, "minutes")?;
        let ss = parse_part(sec, "seconds")?;
        let ff = parse_part(f, "frames")?;
        if ff >= fps {
            return Err(VisualError::InvalidOperation(format!(
                "frame index {ff} out of range for {}",
                frame_rate
            )));
        }
        Ok(Timecode::from_frames(
            (hh * 3600 + mm * 60 + ss) * fps + ff,
            frame_rate,
        ))
    }
}

impl std::fmt::Display for Timecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_hhmmssff())
    }
}

/// A span of time measured in frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Duration {
    /// Length in frames.
    pub frames: u64,
}

impl Duration {
    /// A duration of `frames` frames.
    #[must_use]
    pub fn frames(frames: u64) -> Self {
        Duration { frames }
    }

    /// Duration spanning `seconds` at `frame_rate`.
    #[must_use]
    pub fn from_seconds(seconds: f64, frame_rate: FrameRate) -> Self {
        Duration::frames(frame_rate.frames_for_duration(seconds))
    }

    /// Length in seconds at `frame_rate`.
    #[must_use]
    pub fn as_seconds(self, frame_rate: FrameRate) -> f64 {
        frame_rate.time_of_frame(self.frames)
    }

    /// Whether this duration spans zero frames.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.frames == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timecode() {
        let tc = Timecode::from_frames(24 * 3600 + 24 * 60 + 24 + 12, FrameRate::film());
        assert_eq!(tc.to_string_hhmmssff(), "01:01:01:12");
        assert_eq!(
            Timecode::from_frames(0, FrameRate::film()).to_string(),
            "00:00:00:00"
        );
    }

    #[test]
    fn parses_all_arity_timecodes() {
        let fps = FrameRate::film();
        assert_eq!(
            Timecode::parse("01:01:01:12", fps).unwrap().frames,
            24 * 3600 + 24 * 60 + 24 + 12
        );
        assert_eq!(
            Timecode::parse("01:01:12", fps).unwrap().frames,
            24 * 60 + 24 + 12
        );
        assert_eq!(Timecode::parse("01:12", fps).unwrap().frames, 36);
        assert!(Timecode::parse("00:00:00:24", fps).is_err()); // frame idx == fps
        assert!(Timecode::parse("bogus", fps).is_err());
    }

    #[test]
    fn ntsc_display_rounds_fps() {
        let tc = Timecode::from_frames(45, FrameRate::ntsc());
        // 45 frames at ~29.97 displays as frame 15 of second 1 (30 fps clock).
        assert_eq!(tc.to_string_hhmmssff(), "00:00:01:15");
    }

    #[test]
    fn drop_frame_anchors() {
        let ntsc = FrameRate::ntsc();
        // Minute 0 has no drops.
        assert_eq!(
            Timecode::from_frames(0, ntsc).to_string_drop_frame(),
            "00:00:00;00"
        );
        assert_eq!(
            Timecode::from_frames(1799, ntsc).to_string_drop_frame(),
            "00:00:59;29"
        );
        // First minute boundary drops 00:01:00:00 and :01.
        assert_eq!(
            Timecode::from_frames(1800, ntsc).to_string_drop_frame(),
            "00:01:00;02"
        );
        // The ten-minute boundary drops nothing.
        assert_eq!(
            Timecode::from_frames(17982, ntsc).to_string_drop_frame(),
            "00:10:00;00"
        );
        assert_eq!(
            Timecode::from_frames(17983, ntsc).to_string_drop_frame(),
            "00:10:00;01"
        );
        // Twenty minutes: two skipped ten-minute corrections.
        assert_eq!(
            Timecode::from_frames(35964, ntsc).to_string_drop_frame(),
            "00:20:00;00"
        );
        // Integer rates never drop.
        assert_eq!(
            Timecode::from_frames(1440, FrameRate::film()).to_string_drop_frame(),
            "00:01:00;00"
        );
    }

    #[test]
    fn duration_math() {
        let d = Duration::from_seconds(2.0, FrameRate::film());
        assert_eq!(d.frames, 48);
        assert!((d.as_seconds(FrameRate::film()) - 2.0).abs() < 1e-12);
        assert!(Duration::frames(0).is_zero());
    }

    #[test]
    fn seconds_roundtrip() {
        let tc = Timecode::from_seconds(3.5, FrameRate::film());
        assert!((tc.as_seconds() - 3.5).abs() < 1e-12);
        assert_eq!(tc.frames, 84);
    }
}
