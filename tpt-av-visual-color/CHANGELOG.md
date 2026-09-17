# Changelog — tpt-av-visual-color

All notable changes to the color-science crate. The stack-wide changelog
lives at the repository root.

## [0.1.0] — initial release

### Added
- Transfer functions: sRGB, linear, PQ (ST 2084), HLG (BT.2100), power
  gamma — validated against published anchors.
- Gamut conversion through XYZ with Bradford adaptation; matrices validated
  against published constants and official ACES S-2014-004 AP1↔XYZ
  transforms.
- `GamutMethod::Rolloff` — luminance-preserving out-of-gamut compression
  (CPU + GPU), with `Clip` as the default.
- HDR tone mapping: Reinhard, ACES filmic, custom curves; BT.2408
  reference-white input scaling.
- ACEScc/ACEScct encodings (S-2014-003/S-2016-001) and AP0/AP1 constants.
- `.cube` LUT parsing (1D + 3D) with trilinear sampling; GPU sampling via a
  filterable `rgba16float` 3D texture.
- Fused WGSL `ColorPipeline::apply` with a cached `GpuColorPipeline`; CPU
  reference `apply_pixel` — GPU parity tested.
- Best-effort OCIO config scanner (color spaces + roles) and name-based
  display-pipeline mapping (`display_pipeline_for`).

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
