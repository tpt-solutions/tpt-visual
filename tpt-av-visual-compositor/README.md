# tpt-av-visual-compositor

The GPU-accelerated compositing engine of the TPT AV visual stack. It reads
timeline state, decodes and caches video frames, builds a compositing graph
per frame, and renders it with `wgpu` — designed for real-time playback of
multi-layer timelines.

Part of [tpt-visual](https://github.com/tpt-solutions/tpt-visual). Consumes
[`tpt-av-visual-timeline`](../tpt-av-visual-timeline),
[`tpt-av-visual-effects`](../tpt-av-visual-effects), and
[`tpt-av-visual-color`](../tpt-av-visual-color). Dual-licensed MIT OR
Apache-2.0.

## Features

- **Six-step frame pipeline** — snapshot timeline (lock-free `arc-swap`) →
  fetch frames → upload (YUV→RGB converted on the GPU, BT.709) → build graph
  → execute → advance playhead.
- **Compositing graph** — topological execution with cycle detection;
  nodes: canvas, source, effect chain, transform, 13 blend modes, luma
  matte + chroma-key mask, crossfade/wipe/dissolve transitions with easing,
  output blit (BGRA swap for window surfaces).
- **Asset management** — per-asset caches with LRU GPU residency, background
  prefetch threads (graceful shutdown), CPU proxies for 4K/8K, and decoders:
  procedural, image sequence, and MP4/H.264 via `tpt-kinetix` (feature
  `kinetix`).
- **Export & capture** — `render_frame_rgba`, `render_frame_to_png`,
  `render_frames_to_avi` (built-in MJPEG AVI muxer, no copyleft deps).
- **Windowing** — `GpuContext::instance()` exposes the wgpu instance for
  surface creation; BGRA targets get an automatic R/B swap.

## Install

```toml
[dependencies]
tpt-av-visual-compositor = "0.1"
```

The `kinetix` feature (default) enables H.264 MP4 decoding; disable it with
`default-features = false` to stay codec-free — see `tpt-kinetix`'s
PATENTS.md.

## Example

```rust
use tpt_av_visual_compositor::{default_decoder, TimelineRenderer};
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

let mut session = Session::new("Demo", FrameRate::film(), Resolution::full_hd());
let asset = session.register_asset(timeline::VideoAsset::new(
    AssetId(0), "media/clip.mp4", 240,
    FrameRate::film(), Resolution::full_hd(),
    PixelFormat::Yuv420p, "Rec709",
));
session.tracks[0].insert_clip(Clip::new(session.allocate_clip_id(), asset.id, 0, 0, 240))?;

let mut renderer = TimelineRenderer::headless(session)?.ok_or(compositor::CompositorError::NoDevice)?;
renderer.attach_default_decoders()?;

// Pixels in one call (offscreen render + readback).
let rgba = renderer.render_frame_rgba()?;

// Or export a range straight to video.
let bytes = renderer.render_frames_to_avi("out.avi", 240, 90)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Performance

A 640×360 timeline with an effect renders in ~2.6 ms/frame on a discrete
GPU (see `benches/composite.rs`, `cargo bench -p tpt-av-visual-compositor`).
Texture pooling and LRU frame residency keep long timelines allocation-free
during playback.

## Testing

```sh
cargo test -p tpt-av-visual-compositor
```

GPU tests render real frames and verify them by readback; they skip
automatically on machines without an adapter.

## License

Dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](../LICENSE-MIT) and
[LICENSE-APACHE](../LICENSE-APACHE) at the repository root.
