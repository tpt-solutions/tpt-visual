//! Frame-to-audio-clock alignment.
//!
//! Video timing here is counted in whole frames at a rational [`FrameRate`]
//! (e.g. `30000/1001` for NTSC); audio timing in a host mixer (such as
//! `tpt-audio`) is counted in whole samples at an integer sample rate. This
//! module converts between the two clocks with exact rational arithmetic —
//! no floating-point drift accumulates over a long timeline, which matters
//! most at non-integer rates like 29.97 where `samples_per_video_frame`
//! isn't a whole number.
//!
//! This crate does not depend on `tpt-audio`; the functions below only deal
//! in plain sample/frame counts so either side of an integration can call
//! them without a shared type dependency.

use crate::resolution::FrameRate;

/// The first audio sample index at which video `frame`'s presentation
/// begins, at `sample_rate` samples/second.
///
/// Computed as `floor(frame * sample_rate * den / num)` using exact integer
/// arithmetic (widened to `u128` to avoid overflow at long durations and
/// high sample rates). At an integer frame rate (samples/frame is a whole
/// number) this is exact. At a non-integer frame rate (e.g. NTSC's
/// 30000/1001), a frame's true start time falls between two samples, so
/// this floors to the last sample still inside the *previous* frame's span
/// — [`frame_for_audio_sample`] of that exact sample therefore reports
/// `frame - 1`, not `frame`; use it to find "what frame is playing now",
/// not to invert this function exactly.
#[must_use]
pub fn audio_sample_for_frame(frame: u64, frame_rate: FrameRate, sample_rate: u32) -> u64 {
    let num = frame as u128 * sample_rate as u128 * frame_rate.den as u128;
    (num / frame_rate.num as u128) as u64
}

/// The video frame being presented at audio `sample`, at `sample_rate`
/// samples/second.
///
/// Computed as `floor(sample * num / (sample_rate * den))`. See
/// [`audio_sample_for_frame`]'s docs for the rounding caveat at non-integer
/// frame rates.
#[must_use]
pub fn frame_for_audio_sample(sample: u64, sample_rate: u32, frame_rate: FrameRate) -> u64 {
    let num = sample as u128 * frame_rate.num as u128;
    let den = sample_rate as u128 * frame_rate.den as u128;
    (num / den) as u64
}

/// Audio samples per video frame at `sample_rate`, as an exact ratio
/// (`sample_rate * den / num`). Useful for sizing an audio buffer per
/// rendered video frame; at non-integer frame rates this is not a whole
/// number, so round or accumulate remainder as the caller's buffering
/// strategy requires.
#[must_use]
pub fn samples_per_frame(frame_rate: FrameRate, sample_rate: u32) -> f64 {
    sample_rate as f64 * frame_rate.den as f64 / frame_rate.num as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_frame_rate_is_exact() {
        // 24 fps at 48 kHz: exactly 2000 samples/frame.
        let fr = FrameRate::film();
        assert_eq!(audio_sample_for_frame(0, fr, 48_000), 0);
        assert_eq!(audio_sample_for_frame(1, fr, 48_000), 2_000);
        assert_eq!(audio_sample_for_frame(24, fr, 48_000), 48_000);
        assert_eq!(samples_per_frame(fr, 48_000), 2_000.0);
    }

    #[test]
    fn ntsc_30_frames_take_slightly_over_one_second() {
        // 29.97 fps is slightly slower than 30 fps, so 30 frames span
        // 30 * 1001/30000 = 1.001 s of audio, not exactly 1 s.
        let fr = FrameRate::ntsc();
        assert_eq!(audio_sample_for_frame(30, fr, 48_000), 48_048);
    }

    #[test]
    fn round_trip_never_overshoots_and_is_within_one_frame() {
        let fr = FrameRate::ntsc();
        for frame in [0_u64, 1, 30, 300, 3_000, 90_000] {
            let sample = audio_sample_for_frame(frame, fr, 48_000);
            let back = frame_for_audio_sample(sample, 48_000, fr);
            assert!(
                back <= frame && frame - back <= 1,
                "frame {frame} -> sample {sample} -> {back} (expected {frame} or {})",
                frame.saturating_sub(1)
            );
        }
    }

    #[test]
    fn round_trip_is_exact_at_integer_frame_rate() {
        // Samples/frame is a whole number at an integer fps, so no
        // fractional loss occurs and the round trip is exact.
        let fr = FrameRate::film();
        for frame in [0_u64, 1, 24, 100, 10_000] {
            let sample = audio_sample_for_frame(frame, fr, 48_000);
            assert_eq!(frame_for_audio_sample(sample, 48_000, fr), frame);
        }
    }

    #[test]
    fn frame_for_sample_is_monotonic_and_floors() {
        let fr = FrameRate::film(); // 2000 samples/frame exactly
        assert_eq!(frame_for_audio_sample(0, 48_000, fr), 0);
        assert_eq!(frame_for_audio_sample(1_999, 48_000, fr), 0);
        assert_eq!(frame_for_audio_sample(2_000, 48_000, fr), 1);
        assert_eq!(frame_for_audio_sample(3_999, 48_000, fr), 1);
    }

    #[test]
    fn samples_per_frame_matches_ntsc_reference() {
        let fr = FrameRate::ntsc();
        let spf = samples_per_frame(fr, 48_000);
        assert!((spf - 1_601.6).abs() < 1e-9);
    }
}
