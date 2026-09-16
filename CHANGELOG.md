# Changelog

All notable changes to `tpt-visual` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — initial public milestone

### Added

#### `tpt-av-visual` (facade)
- One-dependency re-export of the whole stack with a curated `prelude`.
- `SessionBuilder` / `ClipSpec` — fluent, validated session construction.
- `probe_gpu()` / `GpuInfo` — adapter, backend, and driver diagnostics.
- `default_decoder` + `TimelineRenderer::attach_default_decoders` — automatic
  MP4(H.264)/procedural decoder selection.

#### `tpt-av-visual-utils`
- `VideoFrame` (planar YUV + packed RGB/RGBA), `PixelFormat` layout helpers,
  `Resolution`, rational `FrameRate`, `Timecode` (non-drop and SMPTE
  drop-frame formatting), `VisualError`.

#### `tpt-av-visual-timeline`
- Non-destructive edit model: `Session` / `Track` / `Clip` / `VideoAsset`.
- Keyframe animation: linear, Catmull-Rom, and bezier interpolation.
- Edit operations (insert/delete/move/split) with full undo/redo history.
- Complete serde support plus JSON document helpers
  (`from_json_path` / `to_json_path`, ...).

#### `tpt-av-visual-compositor`
- wgpu engine: `GpuContext` with loud validation errors, texture pooling,
  pipeline/shader caches.
- YUV→RGB conversion on the GPU (BT.709 limited range, 4:2:0/4:2:2/4:4:4).
- Compositing graph with topological execution and cycle detection; nodes:
  canvas, source, effect chain, transform, blend (13 modes), luma matte +
  chroma-key mask, crossfade/wipe/dissolve transitions with easing, output
  blit (BGRA swap for surfaces).
- `TimelineRenderer` six-step frame pipeline over lock-free session
  snapshots (`arc-swap`).
- `VideoAssetCache` with LRU GPU residency, background prefetch threads, and
  CPU proxy generation; decoders: procedural, image sequence, and
  `tpt-kinetix` MP4/H.264 (feature `kinetix`).
- `render_frame_rgba` / `render_frame_to_png` / `render_frames_to_avi` (with
  the built-in MJPEG `avi::AviWriter`).
- 13 blend modes verified against a CPU reference on the GPU.

#### `tpt-av-visual-color`
- Transfer functions: sRGB, linear, PQ (ST 2084), HLG (BT.2100), power gamma
  — validated against published anchor values.
- Gamut conversion through XYZ with Bradford adaptation; matrices validated
  against published constants and the official ACES S-2014-004 AP1↔XYZ
  transforms.
- HDR tone mapping (Reinhard, ACES filmic, custom curves); BT.2408
  reference-white scaling.
- ACES encodings (ACEScc/ACEScct with S-2016-001 anchors).
- `.cube` LUT parsing (1D + 3D) with trilinear sampling; fused WGSL color
  pipeline (`ColorPipeline::apply`) with a cached `GpuColorPipeline`.
- Best-effort OCIO config scanner (color spaces + roles).

#### `tpt-av-visual-effects`
- Effect trait with CPU reference + WGSL GPU passes; registry resolved from
  timeline effect names.
- Built-ins: gaussian/box/motion blur, sharpen, color correct, levels, tone
  curves (GPU curve texture), chroma key (tolerance/softness/spill), noise
  generation + reduction, vignette.

#### Tooling
- `justfile` (`just gate`, `just demo`, `just render`, `just player`,
  `just docs`) and `.cargo/config.toml` aliases (`cargo lint`, ...).
- GitHub Actions CI: fmt, build, test, clippy `-D warnings`, cargo-deny
  license gate.
- 160+ tests including GPU round-trips (skipped gracefully without an
  adapter) and compile-checked doc examples.

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tpt-solutions/tpt-visual/releases/tag/v0.1.0
