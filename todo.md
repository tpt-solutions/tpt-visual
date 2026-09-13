# tpt-visual — Project Checklist

Tracking all work for `tpt-visual`, the GPU-accelerated video processing layer
of the TPT AV Stack (TPT Solutions). Dual-licensed **MIT OR Apache-2.0**.

---

## Phase 0 — Repo & Tooling Setup

### Repo basics
- [ ] `git init`, initial commit
- [ ] `.gitignore` (Rust `target/`, IDE files, OS cruft)

### Licensing (dual MIT/Apache-2.0)
- [ ] `LICENSE-MIT` (TPT Solutions copyright)
- [ ] `LICENSE-APACHE` (Apache-2.0 text)
- [ ] `Cargo.toml` workspace package sets `license = "MIT OR Apache-2.0"`
- [ ] `README.md` license badges/section reference both licenses

### Workspace scaffold
- [ ] Root `Cargo.toml` — `[workspace]` with 5 members, `[workspace.package]`
      (version, edition 2021, rust-version 1.75, repository), `[workspace.dependencies]`
      per spec §9 (`wgpu 0.19`, `image 0.25`, `serde` w/ derive, `log 0.4`, `tpt-kinetix` git dep)
- [ ] Scaffold `tpt-av-visual-utils/` (`Cargo.toml` + `src/lib.rs`)
- [ ] Scaffold `tpt-av-visual-timeline/` (`Cargo.toml` + `src/lib.rs`)
- [ ] Scaffold `tpt-av-visual-compositor/` (`Cargo.toml` + `src/lib.rs`)
- [ ] Scaffold `tpt-av-visual-color/` (`Cargo.toml` + `src/lib.rs`)
- [ ] Scaffold `tpt-av-visual-effects/` (`Cargo.toml` + `src/lib.rs`)
- [ ] Scaffold `examples/` directory (empty placeholder binaries)
- [ ] `cargo build` succeeds for the empty workspace

### Dependency/license enforcement
- [ ] `deny.toml` — allow `MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib`;
      deny `GPL-2.0, GPL-3.0, LGPL-2.1, LGPL-3.0, AGPL-3.0, MPL-2.0`; `unlicensed = "deny"`
- [ ] `cargo install cargo-deny` documented in CONTRIBUTING/README
- [ ] `cargo deny check` passes locally

### Docs
- [ ] `README.md` — vision, tenets, ecosystem table, data flow diagram (spec §1–2)
- [ ] `DESIGN.md` — full architecture doc (spec content: repo layout, API sketches,
      GPU architecture, roadmap, dependency rules)
- [ ] `CONTRIBUTING.md` — licensing agreement text from spec §10 (contributions are
      dual MIT/Apache-2.0, no copyleft deps, WGSL-only shaders, color code validated
      against reference implementations)

### CI
- [ ] GitHub Actions workflow: `cargo build --workspace`
- [ ] GitHub Actions workflow: `cargo test --workspace`
- [ ] GitHub Actions workflow: `cargo fmt --check`
- [ ] GitHub Actions workflow: `cargo clippy --workspace -- -D warnings`
- [ ] GitHub Actions workflow: `cargo deny check` (license gate, required status check)

---

## Phase 1 — Foundation & Timeline Model

### `tpt-av-visual-utils`
- [ ] `error.rs` — `VisualError` enum (crate-wide error type)
- [ ] `frame.rs` — video frame types (YUV, RGB, RGBA)
- [ ] `pixel_format.rs` — pixel format enums and conversions
- [ ] `resolution.rs` — `Resolution`, aspect ratio, frame rate helpers
- [ ] `time.rs` — timecode, duration, frame counting

### `tpt-av-visual-timeline`
- [ ] `asset.rs` — `VideoAsset` (id, file_path, duration_frames, frame_rate,
      resolution, pixel_format, color_space)
- [ ] `transform.rs` — `Transform` (position, scale, rotation, anchor)
- [ ] `keyframe.rs` — `Keyframe`, `KeyframeTrack`, `InterpolationMethod`
- [ ] `clip.rs` — `Clip` (asset_id, start_frame, source_offset, duration_frames,
      transform, opacity, blend_mode, keyframes, effects)
- [ ] `track.rs` — `Track` (clips, opacity, blend_mode, hidden, locked)
- [ ] `session.rs` — `Session` (id, name, frame_rate, resolution, tracks, metadata)
- [ ] `edit.rs` — edit operations (insert, delete, move, split clip)
- [ ] `history.rs` — undo/redo history stack

### Testing
- [ ] Unit tests: clip splitting at arbitrary frame offsets
- [ ] Unit tests: keyframe interpolation (linear, cubic, bezier)
- [ ] Unit tests: track/session serialization round-trip (serde)

---

## Phase 2 — GPU Compositor Core

### GPU resource management (`tpt-av-visual-compositor/gpu/`)
- [ ] `device.rs` — wgpu instance/adapter/device/queue initialization
- [ ] `texture.rs` — `GpuTexture` struct + `GpuTexture::upload` from `VideoFrame`
- [ ] `pipeline.rs` — render pipeline creation/caching
- [ ] `shader.rs` — WGSL shader loading/compilation management

### Compositing graph
- [ ] `node.rs` — `CompositorNode` trait (`render`, `inputs`)
- [ ] `graph.rs` — `CompositorGraph` (nodes, edges, topological execution order)
- [ ] `nodes/source.rs` — video source node (reads from asset/texture cache)
- [ ] `nodes/transform.rs` — position/scale/rotation node
- [ ] `nodes/blend.rs` — blend modes (normal, multiply, screen, etc.)
- [ ] `nodes/mask.rs` — alpha masking/keying node (basic pass)
- [ ] `nodes/transition.rs` — crossfade, wipe, dissolve
- [ ] `nodes/output.rs` — final output node

### Rendering
- [ ] `Compositor` struct (device, queue, graph, frame_rate, resolution, texture_cache)
- [ ] `renderer.rs` / `TimelineRenderer` (session ref, assets, playhead_frame, compositor)
- [ ] `TimelineRenderer::render_frame` — implement 6-step pipeline (snapshot timeline,
      fetch frames, upload textures, build graph, execute graph, advance playhead)
- [ ] `scheduler.rs` — lock-free state synchronization (Main/UI thread ↔ Render thread)

### Examples
- [ ] `examples/headless_render.rs` — renders a timeline JSON to a video file
- [ ] `examples/simple_player.rs` — plays a single video file
- [ ] `examples/compositor_demo.rs` — composites multiple video layers

---

## Phase 3 — Color Science

### `tpt-av-visual-color`
- [ ] `color_space.rs` — `ColorSpace` enum (Rec709, Rec2020, DciP3, Srgb, Linear,
      Aces, Custom{primaries, white_point})
- [ ] `transfer.rs` — `TransferFunction` enum (Srgb, Linear, Pq, Hlg, Gamma) + conversions
- [ ] `gamut.rs` — gamut mapping/conversion (Rec.709 ↔ Rec.2020 ↔ P3)
- [ ] `hdr.rs` — `ToneMapper` enum (Reinhard, AcesFilmic, Custom curve) + tone mapping
- [ ] `luts.rs` — 1D and 3D LUT loading (via `image`/custom parser) and application
- [ ] `aces.rs` — ACES transforms (IDT/RRT/ODT as applicable)
- [ ] `ocio.rs` — OpenColorIO config compatibility (stub for future work)
- [ ] `ColorPipeline` struct (input/output space+transfer, tone_mapper, lut)
- [ ] `ColorPipeline::apply` — implement 5-step GPU pipeline (linearize, color space
      convert, tone map, apply 3D LUT, convert to output transfer function)

### Validation
- [ ] Cross-check transfer function math against ACES Central reference values
- [ ] Cross-check gamut conversions against OpenColorIO reference configs

---

## Phase 4 — Effects & Advanced Compositing

### `tpt-av-visual-effects`
- [ ] `effect.rs` — `Effect` trait
- [ ] `effects/blur.rs` — Gaussian, box, motion blur
- [ ] `effects/sharpen.rs` — sharpening / edge enhancement
- [ ] `effects/color_correct.rs` — brightness, contrast, saturation, hue
- [ ] `effects/curves.rs` — tone curves (RGB, luminance)
- [ ] `effects/levels.rs` — levels adjustment
- [ ] `effects/chroma_key.rs` — green/blue screen keying
- [ ] `effects/noise.rs` — noise generation and reduction
- [ ] `effects/vignette.rs` — vignette effect

### GPU shaders (WGSL only, per contributing rules)
- [ ] `gpu_shaders/blur.wgsl`
- [ ] `gpu_shaders/color_correct.wgsl`
- [ ] `gpu_shaders/chroma_key.wgsl`

### Advanced compositing
- [ ] Extend `nodes/mask.rs` with full keying support (chroma key integration)
- [ ] Extend `nodes/transition.rs` with configurable transition curves/durations
- [ ] Keyframe animation: bezier interpolation wired into effect parameters via
      `keyframe.rs` / `KeyframeTrack`

---

## Phase 5 — Asset Management & Optimization

- [ ] Integrate `tpt-kinetix` as a dependency for decoding video files into raw frames
- [ ] `VideoAssetCache` — asset metadata, decoded frame buffer, GPU texture map
- [ ] `VideoAssetCache::prefetch` — background decoder thread, decode frame range
- [ ] `VideoAssetCache::get_frame` — cached fast path + synchronous slow-path decode
- [ ] Proxy generation (lower-res transcodes) for smooth 4K/8K playback
- [ ] GPU memory management / texture pooling for `Compositor::texture_cache`
- [ ] Background thread lifecycle management (spawn/join, graceful shutdown)

---

## Ongoing / Cross-Cutting

- [ ] Keep `deny.toml` passing as new dependencies are added
- [ ] Keep README/DESIGN docs in sync with implemented API surface
- [ ] Maintain test coverage per crate (`tests/` dirs already scaffolded in Phase 0)
