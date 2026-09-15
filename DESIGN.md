# tpt-visual — Design

The architecture of `tpt-visual`, the GPU-accelerated video processing layer
of the TPT AV Stack. This document mirrors the implemented system; for the
product vision see [README.md](README.md).

---

## 1. Repository Architecture (Cargo Workspace)

All sub-crates share the `tpt-av-visual-` prefix for ecosystem coherence and
clean namespace resolution on crates.io.

```text
tpt-visual/                        # Workspace root
├── Cargo.toml                     # Workspace manifest
├── deny.toml                      # cargo-deny license audit
├── justfile / .cargo/config.toml  # Task runner + cargo aliases (just gate)
├── LICENSE-MIT / LICENSE-APACHE
├── README.md / DESIGN.md / CONTRIBUTING.md
├── examples/                      # Shared example binaries (facade crate targets)
│   ├── headless_render.rs         # Timeline JSON → MJPEG AVI
│   ├── simple_player.rs           # Windowed playback (winit + surface)
│   ├── compositor_demo.rs         # Multi-layer compositing demo
│
├── tpt-av-visual/                 # Facade: re-exports, SessionBuilder, prelude
│   └── src/{lib,builder}.rs
│
├── tpt-av-visual-utils/           # Shared types, math helpers, error handling
│   └── src/{frame,pixel_format,resolution,time,error}.rs
│
├── tpt-av-visual-timeline/        # Pure data model. Non-destructive edit state.
│   └── src/{session,track,clip,asset,transform,keyframe,edit,history}.rs
│
├── tpt-av-visual-compositor/      # GPU-accelerated rendering engine
│   ├── src/{graph,node,renderer,scheduler,compositor}.rs
│   ├── src/nodes/{source,transform,blend,mask,transition,effect,output}.rs
│   ├── src/gpu/{device,texture,pipeline,shader}.rs
│   ├── src/assets/{cache,decoder,proxies}.rs
│   ├── src/avi.rs                 # Minimal MJPEG AVI muxer (image-crate JPEGs)
│   └── shaders/{yuv_to_rgb,transform,blend,mask,transition,blit}.wgsl
│
├── tpt-av-visual-color/           # Color science and HDR processing
│   ├── src/{color_space,transfer,gamut,hdr,luts,aces,ocio,gpu}.rs
│   └── shaders/color_convert.wgsl
│
└── tpt-av-visual-effects/         # Video effects and filters
    ├── src/effect.rs + src/effects/*.rs
    └── src/gpu_shaders/{blur,sharpen,color_correct,chroma_key,vignette,noise}.wgsl
```

### Dependency graph (crate level)

```text
            tpt-av-visual-utils
             ↑                ↑
   tpt-av-visual-timeline   tpt-av-visual-effects   tpt-av-visual-color
             ↑                                        ↑
             └──────────── tpt-av-visual-compositor ──┘
```

Rules:

- `tpt-av-visual` (the facade) adds no engine logic: it re-exports the
  stack, provides the fluent `SessionBuilder` / `ClipSpec`, `default_decoder`
  and `probe_gpu` helpers, and a `prelude` for one-`use` adoption.
- `utils` depends on nothing GPU-related and can be used anywhere.
- `timeline` is a pure data model (serde only) — no GPU, no I/O.
- `effects` and `color` each talk to `wgpu` directly and depend only on
  `utils` (+ the effects registry used by the compositor).
- `compositor` sits on top: it depends on `utils`, `timeline`, `effects`,
  and `color`... in practice only `effects` + `timeline` today; the color
  pipeline is invoked by hosts (or a future color node) through the `color`
  crate's self-contained GPU API.

## 2. The Timeline Model (`tpt-av-visual-timeline`)

The timeline is the pure data model of a non-destructive edit. It describes
*what* plays, *when*, and *how* — it never touches media.

- **`Session`** — top level: name, `FrameRate` (rational, e.g. 30000/1001),
  `Resolution`, ordered `Track`s (bottom→top), an asset table, and
  `SessionMetadata`. Owns id allocation (tracks, clips, assets) so edits
  after a serde round-trip never collide.
- **`Track`** — clips kept sorted by start frame; overlap insertion is
  rejected; `hidden`/`locked` flags; track-level opacity and blend mode.
- **`Clip`** — `asset_id` + `start_frame`/`source_offset`/`duration_frames`
  (trims are just offset math), `Transform`, opacity, `BlendMode`,
  `KeyframeTrack` list, and `EffectInstance` list (registered effect name +
  numeric parameter bag — the timeline stays decoupled from effect code).
- **`VideoAsset`** — immutable metadata for a media file (path, duration,
  frame rate, resolution, pixel format, color space).
- **`Transform`** — position/scale/rotation about a normalized anchor;
  `to_matrix` produces the affine the compositor inverts for pull-sampling.
- **`KeyframeTrack`** — scalar property animation with linear, cubic
  (Catmull-Rom), and bezier (CSS-style control points on the *left*
  keyframe) interpolation; bezier solving uses Newton-Raphson with a
  bisection fallback.
- **`edit` / `history`** — every operation is a `Reversible` command
  (`InsertClip`, `DeleteClip`, `MoveClip`, `SplitClip`) that validates
  *before* mutating; `History` walks undo/redo with a bounded depth.

Serialization: the whole session round-trips through serde (JSON in the
examples), including id counters.

## 3. The Compositor (`tpt-av-visual-compositor`)

### 3.1 GPU architecture

- **`gpu::GpuContext`** — instance/adapter/device/queue behind `Arc`s, with
  an uncaptured-error handler that panics loudly (fail fast on validation
  bugs). `headless()` probes every backend; GPU-less CI skips gracefully.
- **`gpu::GpuTexture`** — frame upload. RGBA frames upload directly; planar
  YUV (4:2:0/4:2:2/4:4:4, BT.709 limited range) is converted **on the GPU**
  (`yuv_to_rgb.wgsl`) into a packed RGBA texture.
- **`gpu::TexturePool`** — render targets are pooled and recycled across
  frames keyed by (size, usage); no per-frame reallocation.
- **`gpu::PipelineCache`** — one pipeline per (shader, target format),
  created on demand. All node pipelines share a single bind-group layout:
  texture A, sampler, uniform `NodeParams` (96 bytes), texture B.
- **`gpu::ShaderRegistry`** — WGSL embedded with `include_str!`, compiled
  once per module name.

### 3.2 Compositing graph

`CompositorGraph` holds `Box<dyn CompositorNode>`s with explicit slot wiring
(`connect(producer, consumer, slot)`), validates connectivity (unconnected
inputs, cycles via Kahn's algorithm), and executes topologically. Each node
renders into a pooled texture; the last node renders into the caller's
target. Passthrough nodes (sources) forward their views without allocating.

Built-in nodes:

| Node | Purpose |
| :--- | :--- |
| `CanvasNode` | Transparent root the fold starts from. |
| `SourceNode` | Presents an uploaded frame (passthrough view). |
| `EffectNode` | Runs a clip's effect chain via `tpt-av-visual-effects` (ping-pong scratch textures for separable passes). |
| `TransformNode` | Position/scale/rotation by inverse-mapping target UVs through the clip affine; transparent outside the footprint. |
| `BlendNode` | Normal/Multiply/Screen/Overlay/Darken/Lighten/HardLight/Difference/Exclusion with premultiplied-style alpha compositing. |
| `MaskNode` | Luma matte (mask luminance scales alpha) or chroma key mode with spill suppression. |
| `TransitionNode` | Crossfade/wipe/dissolve with configurable easing (linear/smooth/cubic-bezier) evaluated per frame. |
| `OutputNode` | Blits the composed frame into the final target (R/B swap mode for BGRA surfaces). |

### 3.3 Renderer and scheduling

`TimelineRenderer::render_frame` implements the six-step pipeline:

1. **Snapshot** the timeline (`scheduler::RenderStateHandle` — an
   `arc-swap` `ArcSwap<Session>`; the UI thread publishes, the render
   thread loads wait-free).
2. **Fetch** each active clip's frame (`source_frame_at(playhead)`).
3. **Upload** to GPU textures (YUV converted on the GPU).
4. **Build** the graph (canvas → per clip: source → effects → transform →
   blend onto the running canvas; keyframed properties — including effect
   parameters via `effects.<i>.<param>` tracks — are resolved here).
5. **Execute** the graph.
6. **Advance** the playhead.

### 3.4 Asset management

`assets::VideoAssetCache` per asset:

- **Decoders** — `FrameDecoder` trait with `ProceduralDecoder` (test
  pattern), `ImageSequenceDecoder` (numbered PNG/JPEGs), and
  `KinetixDecoder` (MP4/H.264 via `tpt-kinetix`, behind the `kinetix`
  feature; sequential display-order decode with rewind-on-seek).
- **Prefetch** — `prefetch(range)` spawns a background decoder thread whose
  results the render thread drains without blocking; `stop_prefetch` (also
  on `Drop`) sets a stop flag and joins the thread.
- **GPU residency** — LRU-bounded GPU texture map; `get_frame` returns
  cached textures immediately and decodes synchronously on miss.
- **Proxies** — `ProxyConfig` auto-sizes (540p/1080p); frames are
  CPU-rescaled (Triangle filter) and served instead of originals.

## 4. The Color Engine (`tpt-av-visual-color`)

`ColorPipeline` is a five-step conversion; each step has a scalar CPU
reference implementation and lives in one fused WGSL pass
(`color_convert.wgsl`):

1. **Linearize** — sRGB, PQ (ST 2084, 1.0 = 10 000 nits), HLG (BT.2100),
   power gamma; optional `input_linear_scale` (BT.2408: PQ 203 nits → 1.0
   before tone mapping).
2. **Gamut convert** — through CIE XYZ with **Bradford chromatic
   adaptation** between differing white points; matrices are derived from
   chromaticities and validated against published constants (Lindbloom /
   BT.709↔BT.2020↔P3) and the official ACES AP1↔XYZ transforms.
3. **Tone map** — Reinhard, ACES filmic (Narkowicz fit), custom monotone
   curves.
4. **3D LUT** — `.cube` parsing (1D + 3D), trilinear CPU sampling;
   GPU sampling via a `rgba16float` 3D texture (filterable).
5. **Encode** — output transfer function.

`GpuColorPipeline` caches the compiled pipeline + uniform + LUT texture;
`ColorPipeline::apply` is a convenience wrapper. `ocio.rs` documents the
future OpenColorIO compatibility surface.

## 5. Effects (`tpt-av-visual-effects`)

- **`Effect` trait** — `name()`, CPU reference (`apply_cpu` on RGBA8), and
  GPU `passes(width, height) -> Vec<EffectPassDesc>` (shader + shared
  96-byte `EffectParams` uniform + optional 256×1 curve LUT).
- **Registry** — `build_effect(name, &ParamBag)` resolves timeline
  `EffectInstance`s; registered names: `gaussian_blur`, `box_blur`,
  `motion_blur`, `sharpen`, `color_correct`, `levels`, `curves`,
  `chroma_key`, `noise`, `noise_reduction`, `vignette`.
- **GPU** — `EffectRenderer` caches one pipeline per (shader, format) and
  allocates a fresh uniform buffer per pass (queue-timeline writes into a
  shared buffer would race passes in one submit). Separable effects
  ping-pong through scratch textures.
- **Shaders** (WGSL only, per contributing rules): `blur` (separable
  gaussian/box + motion), `sharpen` (unsharp mask), `color_correct`
  (brightness/contrast/saturation/hue + levels + curve texture),
  `chroma_key` (tolerance/softness/spill), `vignette`, `noise`
  (generate + bilateral-ish reduce).

## 6. Real-Time Performance Architecture

- **Frame caching** — decoded CPU frames + LRU GPU textures per asset;
  background prefetch keeps the render thread on the fast path.
- **Thread boundaries** —

  ```text
  Main/UI Thread (can allocate, can block)
  │  mutate timeline → publish snapshot (atomic)
  │  attach assets, start/stop prefetch, configure proxies
  │
  Render Thread (minimal CPU, GPU-bound)
  │  load snapshot (wait-free) → fetch/upload → build graph
  │  → execute graph → submit → present
  ```

- **GPU memory** — texture pooling for intermediates; LRU eviction for
  frame textures; proxy mode reduces 4K/8K upload pressure.

## 7. Roadmap

- **Phase 1 — Foundation & Timeline Model** (done): utils + timeline,
  unit-tested clip splitting/keyframe interpolation/serde round-trips.
- **Phase 2 — GPU Compositor Core** (done): wgpu device management,
  source/transform/blend/mask/transition/output nodes, `TimelineRenderer`
  six-step pipeline, lock-free scheduler, headless CLI renderer.
- **Phase 3 — Color Science** (done): transfer functions, gamut, HDR tone
  mapping, ACES encodings, LUTs, fused GPU pipeline, reference validation.
- **Phase 4 — Effects & Advanced Compositing** (done): effect crates with
  WGSL shaders, masking/keying, transition curves, keyframed effect
  parameters.
- **Phase 5 — Asset Management & Optimization** (done): `tpt-kinetix`
  integration (feature `kinetix`), prefetch + thread lifecycle, proxies,
  texture pooling.
- **Next**: OCIO config ingestion, gamut-mapping (soft clipping) beyond
  clamp, GPU YUV proxy scaling, multi-node NLE polish, audio sync hooks.

## 8. Dependency & Licensing Rules

Allowed (permissive only): internal crates, `tpt-kinetix` (MIT OR
Apache-2.0), `wgpu`, `image`, `serde`, `log`, `pollster`, `arc-swap`,
`bytemuck`, `serde_json`, `anyhow` — all MIT/Apache-2.0 (or BSD/ISC/Zlib).

Banned: `ffmpeg-sys`/`ffmpeg-next` (LGPL/GPL), `opencv` (heavy C++), and
anything GPL/LGPL/AGPL/MPL anywhere in the dependency tree.

Enforced by `deny.toml` (`cargo deny check` in CI): unlicensed = deny,
copyleft = deny, allow-list = MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib.

Note: H.264 decoding (the `kinetix` feature) is patent-encumbered — the
same posture as upstream `tpt-kinetix`, which gates its H.264 decoder
behind a default feature that can be disabled with `--no-default-features`.
