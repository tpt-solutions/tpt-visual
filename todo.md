# tpt-visual — Project Checklist

Tracking all work for `tpt-visual`, the GPU-accelerated video processing layer
of the TPT AV Stack (TPT Solutions). Dual-licensed **MIT OR Apache-2.0**.

---

## Phase 0 — Repo & Tooling Setup

### Repo basics
- [x] `git init`, initial commit
- [x] `.gitignore` (Rust `target/`, IDE files, OS cruft)

### Licensing (dual MIT/Apache-2.0)
- [x] `LICENSE-MIT` (TPT Solutions copyright)
- [x] `LICENSE-APACHE` (Apache-2.0 text)
- [x] `Cargo.toml` workspace package sets `license = "MIT OR Apache-2.0"`
- [x] `README.md` license badges/section reference both licenses

### Workspace scaffold
- [x] Root `Cargo.toml` — `[workspace]` with 5 members, `[workspace.package]`
      (version, edition 2021, rust-version 1.75, repository), `[workspace.dependencies]`
      per spec §9 (`wgpu 0.19`, `image 0.25`, `serde` w/ derive, `log 0.4`,
      `tpt-kinetix` git deps — pinned to core/demux/h264 since upstream has no
      `tpt-kinetix` facade crate)
- [x] Scaffold `tpt-av-visual-utils/` (`Cargo.toml` + `src/lib.rs`)
- [x] Scaffold `tpt-av-visual-timeline/` (`Cargo.toml` + `src/lib.rs`)
- [x] Scaffold `tpt-av-visual-compositor/` (`Cargo.toml` + `src/lib.rs`)
- [x] Scaffold `tpt-av-visual-color/` (`Cargo.toml` + `src/lib.rs`)
- [x] Scaffold `tpt-av-visual-effects/` (`Cargo.toml` + `src/lib.rs`)
- [x] Scaffold `examples/` directory (empty placeholder binaries)
- [x] `cargo build` succeeds for the empty workspace

### Dependency/license enforcement
- [x] `deny.toml` — allow `MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib`
      (+ `CC0-1.0`, `Unicode-3.0` transitively required); copyleft and
      unlicensed denied by cargo-deny v2 default-deny semantics
- [x] `cargo install cargo-deny` documented in CONTRIBUTING/README
- [x] `cargo deny check` passes locally

### Docs
- [x] `README.md` — vision, tenets, ecosystem table, data flow diagram (spec §1–2)
- [x] `DESIGN.md` — full architecture doc (spec content: repo layout, API sketches,
      GPU architecture, roadmap, dependency rules)
- [x] `CONTRIBUTING.md` — licensing agreement text from spec §10 (contributions are
      dual MIT/Apache-2.0, no copyleft deps, WGSL-only shaders, color code validated
      against reference implementations)

### CI
- [x] GitHub Actions workflow: `cargo build --workspace`
- [x] GitHub Actions workflow: `cargo test --workspace`
- [x] GitHub Actions workflow: `cargo fmt --check`
- [x] GitHub Actions workflow: `cargo clippy --workspace -- -D warnings`
- [x] GitHub Actions workflow: `cargo deny check` (license gate, required status check)

---

## Phase 1 — Foundation & Timeline Model

### `tpt-av-visual-utils`
- [x] `error.rs` — `VisualError` enum (crate-wide error type)
- [x] `frame.rs` — video frame types (YUV, RGB, RGBA)
- [x] `pixel_format.rs` — pixel format enums and conversions
- [x] `resolution.rs` — `Resolution`, aspect ratio, frame rate helpers
- [x] `time.rs` — timecode, duration, frame counting

### `tpt-av-visual-timeline`
- [x] `asset.rs` — `VideoAsset` (id, file_path, duration_frames, frame_rate,
      resolution, pixel_format, color_space)
- [x] `transform.rs` — `Transform` (position, scale, rotation, anchor)
- [x] `keyframe.rs` — `Keyframe`, `KeyframeTrack`, `InterpolationMethod`
- [x] `clip.rs` — `Clip` (asset_id, start_frame, source_offset, duration_frames,
      transform, opacity, blend_mode, keyframes, effects)
- [x] `track.rs` — `Track` (clips, opacity, blend_mode, hidden, locked)
- [x] `session.rs` — `Session` (id, name, frame_rate, resolution, tracks, metadata)
- [x] `edit.rs` — edit operations (insert, delete, move, split clip)
- [x] `history.rs` — undo/redo history stack

### Testing
- [x] Unit tests: clip splitting at arbitrary frame offsets
- [x] Unit tests: keyframe interpolation (linear, cubic, bezier)
- [x] Unit tests: track/session serialization round-trip (serde)

---

## Phase 2 — GPU Compositor Core

### GPU resource management (`tpt-av-visual-compositor/gpu/`)
- [x] `device.rs` — wgpu instance/adapter/device/queue initialization
- [x] `texture.rs` — `GpuTexture` struct + `GpuTexture::upload` from `VideoFrame`
- [x] `pipeline.rs` — render pipeline creation/caching
- [x] `shader.rs` — WGSL shader loading/compilation management

### Compositing graph
- [x] `node.rs` — `CompositorNode` trait (`render`, `inputs`)
- [x] `graph.rs` — `CompositorGraph` (nodes, edges, topological execution order)
- [x] `nodes/source.rs` — video source node (reads from asset/texture cache)
- [x] `nodes/transform.rs` — position/scale/rotation node
- [x] `nodes/blend.rs` — blend modes (normal, multiply, screen, etc.)
- [x] `nodes/mask.rs` — alpha masking/keying node (basic pass)
- [x] `nodes/transition.rs` — crossfade, wipe, dissolve
- [x] `nodes/output.rs` — final output node

### Rendering
- [x] `Compositor` struct (device, queue, graph, frame_rate, resolution, texture_cache)
- [x] `renderer.rs` / `TimelineRenderer` (session ref, assets, playhead_frame, compositor)
- [x] `TimelineRenderer::render_frame` — implement 6-step pipeline (snapshot timeline,
      fetch frames, upload textures, build graph, execute graph, advance playhead)
- [x] `scheduler.rs` — lock-free state synchronization (Main/UI thread ↔ Render thread)

### Examples
- [x] `examples/headless_render.rs` — renders a timeline JSON to a video file
      (MJPEG AVI writer, no copyleft dependencies)
- [x] `examples/simple_player.rs` — plays a single video file (winit + surface,
      H.264 via the `kinetix` feature, procedural fallback)
- [x] `examples/compositor_demo.rs` — composites multiple video layers

---

## Phase 3 — Color Science

### `tpt-av-visual-color`
- [x] `color_space.rs` — `ColorSpace` enum (Rec709, Rec2020, DciP3, Srgb, Linear,
      Aces, Custom{primaries, white_point})
- [x] `transfer.rs` — `TransferFunction` enum (Srgb, Linear, Pq, Hlg, Gamma) + conversions
- [x] `gamut.rs` — gamut mapping/conversion (Rec.709 ↔ Rec.2020 ↔ P3)
- [x] `hdr.rs` — `ToneMapper` enum (Reinhard, AcesFilmic, Custom curve) + tone mapping
- [x] `luts.rs` — 1D and 3D LUT loading (via `image`/custom parser) and application
- [x] `aces.rs` — ACES transforms (IDT/RRT/ODT as applicable)
- [x] `ocio.rs` — OpenColorIO config compatibility (stub for future work)
- [x] `ColorPipeline` struct (input/output space+transfer, tone_mapper, lut)
- [x] `ColorPipeline::apply` — implement 5-step GPU pipeline (linearize, color space
      convert, tone map, apply 3D LUT, convert to output transfer function)

### Validation
- [x] Cross-check transfer function math against ACES Central reference values
      (ST 2084 anchors, ACEScct S-2016-001 anchors, sRGB IEC anchors)
- [x] Cross-check gamut conversions against OpenColorIO reference configs
      (published Lindbloom matrices; official S-2014-004 AP1↔XYZ constants)

---

## Phase 4 — Effects & Advanced Compositing

### `tpt-av-visual-effects`
- [x] `effect.rs` — `Effect` trait
- [x] `effects/blur.rs` — Gaussian, box, motion blur
- [x] `effects/sharpen.rs` — sharpening / edge enhancement
- [x] `effects/color_correct.rs` — brightness, contrast, saturation, hue
- [x] `effects/curves.rs` — tone curves (RGB, luminance)
- [x] `effects/levels.rs` — levels adjustment
- [x] `effects/chroma_key.rs` — green/blue screen keying
- [x] `effects/noise.rs` — noise generation and reduction
- [x] `effects/vignette.rs` — vignette effect

### GPU shaders (WGSL only, per contributing rules)
- [x] `gpu_shaders/blur.wgsl`
- [x] `gpu_shaders/color_correct.wgsl`
- [x] `gpu_shaders/chroma_key.wgsl`
- [x] (+ `gpu_shaders/sharpen.wgsl`, `vignette.wgsl`, `noise.wgsl`)

### Advanced compositing
- [x] Extend `nodes/mask.rs` with full keying support (chroma key integration)
- [x] Extend `nodes/transition.rs` with configurable transition curves/durations
- [x] Keyframe animation: bezier interpolation wired into effect parameters via
      `keyframe.rs` / `KeyframeTrack`

---

## Phase 5 — Asset Management & Optimization

- [x] Integrate `tpt-kinetix` as a dependency for decoding video files into raw frames
      (git deps on core/demux/h264, feature `kinetix`, default on)
- [x] `VideoAssetCache` — asset metadata, decoded frame buffer, GPU texture map
- [x] `VideoAssetCache::prefetch` — background decoder thread, decode frame range
- [x] `VideoAssetCache::get_frame` — cached fast path + synchronous slow-path decode
- [x] Proxy generation (lower-res transcodes) for smooth 4K/8K playback
- [x] GPU memory management / texture pooling for `Compositor::texture_cache`
- [x] Background thread lifecycle management (spawn/join, graceful shutdown)

---

## Phase 6 — Review, Adoption & Tooling

### Review fixes
- [x] Fix overlay/hard-light blend formulas in `blend.wgsl` (wrong branch
      terms/swapped conditions); verify all nine modes against a CPU
      reference with a GPU end-to-end test (`tests/blend_modes.rs`)
- [x] Remove leftover debug scaffolding (env-gated prints) from the
      compositor nodes and dedupe the `NodeFrame` effect-runner shim
- [x] Configure the compositor from the session snapshot before every
      render/export (resolution + frame rate) — fixes black-frame renders
      into targets smaller than the default
- [x] Readback row alignment in examples/tests (256-byte
      `COPY_BYTES_PER_ROW_ALIGNMENT`)

### Adoption (facade crate)
- [x] `tpt-av-visual` facade crate — one dependency re-exporting the whole
      stack with a curated `prelude`
- [x] `SessionBuilder` / `ClipSpec` fluent construction (no manual id
      bookkeeping, validated on `build`)
- [x] `probe_gpu()` / `GpuInfo` — environment diagnostics
- [x] `default_decoder` + `TimelineRenderer::attach_default_decoders` —
      automatic MP4(H.264)/procedural decoder selection
- [x] `TimelineRenderer::render_frame_rgba` — one-call offscreen pixel access
- [x] `TimelineRenderer::render_frames_to_avi` + library `avi::AviWriter` —
      three-line headless video export (examples refactored onto it)
- [x] `Session::from_json` / `from_json_path` / `to_json_string` /
      `to_json_path` JSON document helpers
- [x] `SolidDecoder` — solid-color test/fill source in the compositor
- [x] `--help` flag for the `headless_render` example
- [x] Facade integration tests (`tpt-av-visual/tests/facade.rs`): builder
      validation, RGBA render, AVI export, JSON round-trip

### Tooling
- [x] `justfile` — `just gate` (fmt+clippy+build+test+deny), `just demo`,
      `just render`, `just player`, `just docs`
- [x] `.cargo/config.toml` aliases — `cargo lint`, `cargo check-fmt`,
      `cargo demo`, `cargo render`
- [x] Compile-checked doc examples (`cargo test --doc`) on the facade and
      builder

### Color science extras
- [x] `ocio.rs` upgraded from stub to a best-effort OCIO config scanner
      (color spaces + roles) with tests; transform compilation stays future
      work

---

## Phase 7 — Backlog (identified during review, not yet started)

### Engine
- [ ] OCIO v2 transform compilation (file transforms → `ColorPipeline`),
      building on the `ocio.rs` config scanner
- [ ] Gamut mapping beyond clamping (soft/rolloff clipping, e.g. BT.2408 or
      per-channel knee) as a `ColorPipeline` option
- [ ] GPU-side proxy scaling (currently CPU Triangle resize)
- [ ] Additional blend modes (Hue/Saturation/Color/Luminosity) and
      per-clip blend opacity curves
- [ ] Drop-frame timecode (29.97 DF) display support alongside non-drop
- [ ] Audio sync hooks with `tpt-audio` (frame-to-audio-clock alignment)

### Quality & performance
- [ ] Criterion benchmark suite: composite frame time, blur throughput,
      color pipeline pass, AVI export
- [ ] Fuzz targets for the `.cube` parser and AVI reader-side assumptions
- [ ] Multi-track opacity/blend interaction tests (track opacity × clip
      opacity × blend mode matrix)
- [ ] MSRV job in CI pinning Rust 1.75 (current gates run on stable only)

### Packaging & adoption
- [ ] Publish crates to crates.io (`tpt-av-visual` facade + members) and add
      `CHANGELOG.md` + release automation (e.g. release-plz, matching
      tpt-kinetix)
- [ ] `docs/session-json.md` — documented JSON schema for timeline documents
      with a committed `examples/session.json`
- [ ] `render_frame_to_png` screenshot convenience (RGBA → file in one call)
- [ ] Decide Cargo.lock policy for the workspace (currently ignored; commit
      a locked file for the example binaries)
- [ ] Optional `winit`/`image` feature gates so library users do not inherit
      example-only dependencies

---

## Ongoing / Cross-Cutting

- [x] Keep `deny.toml` passing as new dependencies are added
- [x] Keep README/DESIGN docs in sync with implemented API surface
- [x] Maintain test coverage per crate (`tests/` dirs already scaffolded in Phase 0)
