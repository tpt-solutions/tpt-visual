//! `tpt-av-visual-utils` — shared types, math helpers, and error handling for
//! the TPT AV visual stack.
//!
//! This crate is the common vocabulary spoken by every other `tpt-av-visual-*`
//! crate: video frames, pixel formats, resolutions, frame rates, timecodes,
//! frame-to-audio-sample conversion ([`audio_sync`]), and the crate-wide
//! [`VisualError`] type. It is deliberately CPU-only — no GPU dependencies —
//! so it can be used from any layer of a host application.
//!
//! # Ecosystem
//!
//! `tpt-kinetix` decodes media files into raw frames; those frames are
//! converted into this crate's [`frame::VideoFrame`] at the boundary so the
//! rest of the visual stack never depends on a specific decoder.

pub mod audio_sync;
pub mod error;
pub mod frame;
pub mod pixel_format;
pub mod resolution;
pub mod time;

pub use audio_sync::{audio_sample_for_frame, frame_for_audio_sample, samples_per_frame};
pub use error::VisualError;
pub use frame::VideoFrame;
pub use pixel_format::PixelFormat;
pub use resolution::{FrameRate, Resolution};
pub use time::Timecode;

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, VisualError>;
