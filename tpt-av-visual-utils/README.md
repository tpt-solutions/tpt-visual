# tpt-av-visual-utils

Shared vocabulary for the TPT AV visual stack: video frames, pixel formats,
resolutions, frame rates, timecodes, and the crate-wide error type. Pure CPU,
no GPU dependencies — safe to use from any layer of a host application.

Part of [tpt-visual](https://github.com/tpt-solutions/tpt-visual). Every other
`tpt-av-visual-*` crate speaks this crate's types. Dual-licensed MIT OR
Apache-2.0.

## Features

- **`VideoFrame`** — decoded frame container: planar YUV (4:2:0/4:2:2/4:4:4)
  and packed RGB/BGR/RGBA, with per-plane access and a BT.709 CPU conversion
  to RGBA.
- **`PixelFormat`** — layout metadata: plane count, bit depth, plane
  dimensions/sizes/offsets, expected buffer lengths, validation.
- **`Resolution`** — dimensions with aspect ratio, containment, and
  aspect-preserving fit.
- **`FrameRate`** — rational rates (`30000/1001` NTSC included) with frame
  ↔ second conversions.
- **`Timecode`** — SMPTE `HH:MM:SS:FF` formatting/parsing (non-drop) plus
  **drop-frame** (`HH:MM:SS;FF`) display for NTSC-family rates.
- **`VisualError`** — the stack-wide error enum.

## Install

```toml
[dependencies]
tpt-av-visual-utils = "0.1"
```

## Example

```rust
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution, Timecode, VideoFrame};

// Build a 2×2 RGBA frame.
let frame = VideoFrame::from_rgba(2, 2, vec![255; 16], 0);

// Timecodes count exact frames and format non-drop or drop-frame.
let tc = Timecode::from_frames(1800, FrameRate::ntsc());
assert_eq!(tc.to_string_hhmmssff(), "00:01:00:00"); // NDF
assert_eq!(tc.to_string_drop_frame(), "00:01:00;02"); // DF skips :00/:01

// Resolutions fit inside other resolutions, preserving aspect.
let fit = Resolution::full_hd().scale_to_fit(Resolution::new(640, 640).unwrap());
assert_eq!(fit, Resolution::new(640, 360).unwrap());
```

## Testing

```sh
cargo test -p tpt-av-visual-utils
```

## License

Dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](../LICENSE-MIT) and
[LICENSE-APACHE](../LICENSE-APACHE) at the repository root.
