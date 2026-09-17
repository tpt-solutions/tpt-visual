# tpt-av-visual-effects

GPU-accelerated video effects for the TPT AV visual stack. Every effect ships
a CPU reference implementation (validation + software fallback) and one or
more WGSL GPU passes sharing a single uniform layout, so any effect chain
runs through one renderer.

Part of [tpt-visual](https://github.com/tpt-solutions/tpt-visual). Consumed by
[`tpt-av-visual-compositor`](../tpt-av-visual-compositor) and driven by
timeline effect instances. Dual-licensed MIT OR Apache-2.0.

## Effects

| Name | What it does |
| :--- | :--- |
| `gaussian_blur` | Separable Gaussian blur (two-pass, any radius). |
| `box_blur` | Separable box blur. |
| `motion_blur` | Directional smear (angle + length). |
| `sharpen` | Unsharp-mask edge enhancement. |
| `color_correct` | Brightness / contrast / saturation / hue. |
| `levels` | In black/white, gamma, out black/white. |
| `curves` | Baked 256-entry tone curves (GPU curve texture). |
| `chroma_key` | Green/blue screen keying with softness + spill suppression. |
| `noise` | Deterministic film-grain generation (monochrome or RGB). |
| `noise_reduction` | Bilateral-weighted smoothing that preserves edges. |
| `vignette` | Radial darkening with radius/softness control. |

## Install

```toml
[dependencies]
tpt-av-visual-effects = "0.1"
```

## Example

Build an effect from the registry (names match timeline effect instances)
and apply it on the CPU:

```rust
use std::collections::BTreeMap;
use tpt_av_visual_effects::{build_effect, Effect, ParamBag};

let mut params = ParamBag::new();
params.insert("radius".into(), 3.0);
let blur = build_effect("gaussian_blur", &params)?;

let mut rgba = vec![128_u8; 32 * 32 * 4];
blur.apply_cpu(&mut rgba, 32, 32);

// The same effect describes its GPU passes:
let passes = blur.passes(32, 32);
assert!(!passes.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

`EffectRenderer` executes effect passes on a `wgpu` device with cached
pipelines — the compositor's effect node uses exactly this. GPU parity with
the CPU references is covered by integration tests (skipped gracefully on
machines without an adapter).

## License

Dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](../LICENSE-MIT) and
[LICENSE-APACHE](../LICENSE-APACHE) at the repository root.
