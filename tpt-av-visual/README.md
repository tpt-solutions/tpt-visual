# tpt-av-visual

The facade crate of the TPT AV visual stack: **one dependency** that re-exports
the timeline, GPU compositor, color science, and effects crates, plus
batteries-included helpers so the common path stays short.

Part of [tpt-visual](https://github.com/tpt-solutions/tpt-visual). Dual-licensed
MIT OR Apache-2.0.

## Features

- **Curated `prelude`** — `Session`, `Clip`, `TimelineRenderer`,
  `FrameRate`, `VideoFrame`, and friends in one `use`.
- **`SessionBuilder` / `ClipSpec`** — fluent, validated session construction
  (tracks and assets referenced by position, fades and effects inline).
- **`probe_gpu()`** — adapter, backend, and driver diagnostics.
- **`default_decoder`** — `.mp4`/`.mov` → `tpt-kinetix` (feature `kinetix`),
  everything else → procedural test pattern.
- **`TimelineRenderer`** — six-step frame pipeline with `render_frame`
  (surface), `render_frame_rgba` (pixels), `render_frame_to_png`
  (screenshot), and `render_frames_to_avi` (MJPEG video export).

## Install

```toml
[dependencies]
tpt-av-visual = "0.1"
```

## Example

```rust
use tpt_av_visual::prelude::*;

let session = SessionBuilder::new("Demo", FrameRate::film(), Resolution::full_hd())
    .add_video_asset_with(
        "procedural://intro",
        240,
        FrameRate::film(),
        Resolution::full_hd(),
        PixelFormat::Rgba8,
        "Rec709",
    )
    .add_track("Tint")
    .add_clip(
        ClipSpec::new(1, 0, 0)
            .duration(240)
            .blend_mode(BlendMode::Screen)
            .opacity(0.8)
            .fade_in(24, FadeCurve::Smooth),
    )
    .build()?;

let mut renderer = TimelineRenderer::headless(session)?.ok_or(Error::NoDevice)?;
renderer.attach_default_decoders()?;
let bytes = renderer.render_frames_to_avi("demo.avi", 240, 90)?;
println!("wrote demo.avi ({bytes} bytes)");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Requires a GPU (Vulkan/Metal/D3D12 via `wgpu`); `probe_gpu()` tells you what
you got.

## Testing

```sh
cargo test -p tpt-av-visual
```

GPU-dependent tests skip automatically on machines without an adapter.

## License

Dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](../LICENSE-MIT) and
[LICENSE-APACHE](../LICENSE-APACHE) at the repository root.
