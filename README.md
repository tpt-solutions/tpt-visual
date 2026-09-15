# tpt-visual

**A memory-safe, GPU-accelerated video processing engine. Real-time compositing, color science, and effects. The brain of the TPT AV visual stack.**

**Status:** Early-stage / Pre-1.0
**License:** MIT OR Apache-2.0
**Ecosystem:** [TPT Solutions Open Source](https://opensource.tptsolutions.co.nz/)

---

## Vision

`tpt-visual` is the **video processing layer** of the TPT AV Stack. It provides
the foundational crates required to build non-destructive video editors,
motion graphics tools, live streaming applications, and visual effects
pipelines.

The Rust graphics ecosystem has excellent primitives (`wgpu` for GPU
abstraction, `image` for basic manipulation) but **no unified framework** for
professional video processing: no standard way to represent a non-destructive
video edit, no GPU-accelerated compositor that reads from a timeline, and no
pure-Rust color science engine that handles HDR, wide gamut, and broadcast
standards. `tpt-visual` fills that gap — it turns raw video frames from
[`tpt-kinetix`](https://github.com/tpt-solutions/tpt-kinetix) into a
professional, non-destructive video application.

### Core Tenets

1. **Strictly Non-Destructive** — original media files are never mutated; the
   engine manages metadata, edit decisions, and real-time GPU transformations.
2. **GPU-Accelerated** — all compositing, color transforms, and effects run on
   the GPU via `wgpu` (WebGPU).
3. **Real-Time Safe** — designed for 60 fps+ playback of 4K/8K timelines with
   multiple effect layers.
4. **Professional Color Science** — Rec.709/Rec.2020/DCI-P3, HDR (PQ, HLG),
   ACES encodings, 3D LUTs; every transfer function is validated against
   published reference values.
5. **Clean Thread Boundaries** — strict separation between the Main/UI thread
   (allocates, blocks) and the render thread (GPU-bound, lock-free snapshots).
6. **Permissive Licensing Only** — no GPL/LGPL/AGPL/MPL dependencies,
   enforced by `cargo-deny` in CI.
7. **Composable Architecture** — each sub-crate is independently useful.

## Ecosystem

`tpt-visual` sits in the **Processing Layer** of the TPT AV Stack, consuming
decoded video frames from `tpt-kinetix` and outputting to the display or
encoder.

| Crate | Role | Relationship to `tpt-visual` |
| :--- | :--- | :--- |
| **`tpt-kinetix`** | Media containers, video codecs | Decodes video files into raw frames (YUV/RGB). `tpt-visual` consumes these frames. |
| **`tpt-visual`** | **Video processing (this repo)** | Manages the non-destructive timeline, composites video on the GPU, applies color grading and effects. |
| **`tpt-audio`** | Audio processing | Provides synchronized audio playback. `tpt-visual` syncs video frames to the audio timeline. |

## Crates

| Crate | Purpose |
| :--- | :--- |
| **[`tpt-av-visual`](tpt-av-visual)** | **Facade — start here.** One dependency re-exporting the whole stack, plus `SessionBuilder`, `probe_gpu`, `default_decoder`, and a curated `prelude`. |
| [`tpt-av-visual-utils`](tpt-av-visual-utils) | Shared vocabulary: `VideoFrame`, `PixelFormat`, `Resolution`, `FrameRate`, `Timecode`, `VisualError`. |
| [`tpt-av-visual-timeline`](tpt-av-visual-timeline) | Pure non-destructive edit model: `Session` / `Track` / `Clip` / `VideoAsset`, keyframe animation, edit operations, undo/redo history. Serde-serializable. |
| [`tpt-av-visual-compositor`](tpt-av-visual-compositor) | GPU rendering engine: lock-free state sync, compositing graph, transform/blend/mask/transition nodes, asset caches with prefetching and proxies, MJPEG AVI export. |
| [`tpt-av-visual-color`](tpt-av-visual-color) | Color science: transfer functions (sRGB/PQ/HLG/gamma), gamut conversion with Bradford adaptation, HDR tone mapping, ACES encodings, `.cube` LUTs — CPU reference + one fused GPU pass. |
| [`tpt-av-visual-effects`](tpt-av-visual-effects) | Effects: blur/sharpen/color-correct/levels/curves/chroma-key/noise/vignette — CPU reference + WGSL, shared uniform layout, effect registry. |

## Data Flow

```text
tpt-kinetix (decodes video files → raw frames YUV/RGB)
        ↓
tpt-av-visual-timeline (manages non-destructive edit state)
        ↓
tpt-av-visual-compositor (GPU-based rendering, applies effects)
        ↓
tpt-av-visual-color (color space conversion, HDR tone mapping)
        ↓
Display / Encoder (outputs to screen or file)
```

## Quick Start

Add the facade to your `Cargo.toml`:

```toml
[dependencies]
tpt-av-visual = "0.1"
```

Build a timeline and render it to an MJPEG AVI (requires a GPU):

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
    .add_clip(ClipSpec::new(0, 0, 0).duration(240))
    .build()?;

let mut renderer = TimelineRenderer::headless(session)?.ok_or(
    tpt_av_visual::Error::NoDevice,
)?;
renderer.attach_default_decoders()?;
let bytes = renderer.render_frames_to_avi("demo.avi", 240, 90)?;
println!("wrote demo.avi ({bytes} bytes)");
# Ok::<(), tpt_av_visual::Error>(())
```

Procedural sources need no media files; point `add_video_asset_with` at a
real `.mp4` and the default decoders use `tpt-kinetix` (H.264).

### Command-line demos

```sh
cargo demo -- --frames 90 --out demo.avi     # multi-layer compositing demo
cargo render session.json out.avi 240        # timeline JSON -> AVI
cargo run --release -p tpt-av-visual --example simple_player -- clip.mp4
```

## Tooling

- **`justfile`** — `just gate` runs everything CI runs (fmt, clippy, build,
  test, deny); `just demo`, `just render <json>`, `just player <file>`,
  `just docs`. Install with `cargo install just` or read the recipes and run
  the commands by hand.
- **`.cargo/config.toml` aliases** — `cargo lint` (clippy `-D warnings`),
  `cargo check-fmt`, `cargo demo`, `cargo render`.
- **JSON sessions** — `Session::to_json_path` / `Session::from_json_path`
  serialize the whole edit document; see the `headless_render` example.
- **GPU probe** — `tpt_av_visual::probe_gpu()` returns the adapter, backend,
  and driver the engine will use, or `None` on GPU-less machines.

## GPU Requirement

The compositor, color pipeline, and effects run on the GPU via `wgpu`
(Vulkan / Metal / D3D12 / GL). GPU-dependent tests skip themselves
automatically when no adapter is available (e.g. GPU-less CI runners); all
core logic also ships a CPU reference implementation.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions. See [CONTRIBUTING.md](CONTRIBUTING.md).
