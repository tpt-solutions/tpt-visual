# tpt-av-visual-color

Color science for the TPT AV visual stack: transfer functions, gamut
conversion, HDR tone mapping, ACES encodings, and LUT processing — each with
a scalar CPU reference implementation (validated against published reference
values) and a single fused GPU pass.

Part of [tpt-visual](https://github.com/tpt-solutions/tpt-visual).
Dual-licensed MIT OR Apache-2.0.

## Features

- **Transfer functions** — sRGB, linear, PQ (ST 2084), HLG (BT.2100), power
  gamma; encode/decode validated against ITU/IEC/SMPTE anchor values.
- **Gamut conversion** — Rec.709 ↔ Rec.2020 ↔ DCI-P3 ↔ ACES AP1 through CIE
  XYZ with **Bradford chromatic adaptation**; matrices match published
  constants and the official ACES S-2014-004 AP1↔XYZ transforms.
- **Out-of-gamut handling** — hard clip, or **luminance-preserving rolloff**
  (brightness kept, saturation drops — recommended for Rec.2020 → Rec.709
  delivery).
- **HDR tone mapping** — Reinhard, ACES filmic (Narkowicz fit), custom
  curves; BT.2408 reference-white scaling for PQ/HLG input.
- **ACES** — ACEScc/ACEScct encodings (S-2014-003/S-2016-001) and AP0/AP1
  constants.
- **LUTs** — `.cube` parsing (1D + 3D), trilinear CPU sampling, GPU sampling
  via a filterable `rgba16float` 3D texture.
- **OCIO (preview)** — best-effort config scanner (color spaces + roles) and
  name-based mapping onto known color spaces; full transform compilation is
  future work.

## Install

```toml
[dependencies]
tpt-av-visual-color = "0.1"
```

## Example

```rust
use tpt_av_visual_color::{
    ColorPipeline, ColorSpace, GamutMethod, TransferFunction, ToneMapper,
};

// HDR10 (Rec.2020 PQ) → sRGB display, reference white per BT.2408.
let pipeline = ColorPipeline::new(
    ColorSpace::Rec2020,
    TransferFunction::Pq,
    ColorSpace::Srgb,
    TransferFunction::Srgb,
)
.with_input_linear_scale(1.0 / 0.0203) // 203 nits -> 1.0
.with_tone_mapper(ToneMapper::AcesFilmic)
.with_gamut_method(GamutMethod::Rolloff);

// CPU reference path: code-domain color in, display code out.
let display = pipeline.apply_pixel([0.508, 0.508, 0.508]); // PQ 100-nit grey
```

`ColorPipeline::apply` runs the identical five steps as one fused WGSL pass
on a `wgpu` device (`GpuColorPipeline` caches the compiled pipeline, uniform
block, and LUT texture). GPU parity with the CPU reference is covered by
tests.

## Validation

Transfer functions are checked against ST 2084, BT.2100, and IEC 61966-2-1
anchors; ACEScct against S-2016-001; matrices against Lindbloom's published
constants and the official ACES AP1↔XYZ transforms. Contribution rule: new
color math ships with reference-value tests (see
[CONTRIBUTING](../CONTRIBUTING.md)).

## License

Dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](../LICENSE-MIT) and
[LICENSE-APACHE](../LICENSE-APACHE) at the repository root.
