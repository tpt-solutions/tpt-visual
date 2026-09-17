# Changelog — tpt-av-visual-utils

All notable changes to the shared-types crate. The stack-wide changelog
lives at the repository root.

## [Unreleased]

### Added
- `audio_sync` — exact-rational frame ↔ audio-sample conversion
  (`audio_sample_for_frame`, `frame_for_audio_sample`, `samples_per_frame`)
  for aligning a `FrameRate`-timed video timeline against a sample-rate-timed
  audio clock (e.g. `tpt-audio`), without a dependency on any audio crate.

## [0.1.0] — initial release

### Added
- `VideoFrame` — planar YUV (4:2:0/4:2:2/4:4:4) and packed RGB/BGR/RGBA
  container with per-plane access and BT.709 CPU → RGBA conversion.
- `PixelFormat` — plane counts, bit depth, plane dimensions/sizes/offsets,
  expected buffer lengths, validation.
- `Resolution` — aspect ratio, containment, aspect-preserving fit.
- `FrameRate` — rational rates (30000/1001 included) with frame ↔ second
  conversions.
- `Timecode` — `HH:MM:SS:FF` parse/format plus SMPTE drop-frame
  (`HH:MM:SS;FF`) display for NTSC-family rates.
- `Duration` — frame-count spans with second conversions.
- `VisualError` — the stack-wide error enum.

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
