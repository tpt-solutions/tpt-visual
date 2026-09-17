# Changelog — tpt-av-visual-compositor

All notable changes to the GPU compositing engine. The stack-wide changelog
lives at the repository root.

## [0.1.0] — initial release

### Added
- `GpuContext` — instance/adapter/device/queue with loud validation errors
  and headless probing.
- `TimelineRenderer` six-step frame pipeline over lock-free session
  snapshots (`arc-swap`).
- Compositing graph: topological execution, cycle detection, canvas /
  source / effect-chain / transform / blend / luma-matte / chroma-key /
  transition / output nodes.
- 13 blend modes (including the HSL family), verified GPU-vs-CPU.
- YUV→RGB conversion on the GPU (BT.709 limited range, 4:2:0/4:2:2/4:4:4).
- `render_frame_rgba`, `render_frame_to_png`, `render_frames_to_avi` with
  the built-in MJPEG `avi::AviWriter`.
- `VideoAssetCache` — LRU GPU residency, background prefetch with graceful
  shutdown, CPU proxies; decoders: procedural, image sequence, and
  `tpt-kinetix` MP4/H.264 (feature `kinetix`).
- Texture pooling and pipeline/shader caches; per-frame GPU readback helpers.
- Criterion benchmark (`composite_640x360_vignette`).

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
