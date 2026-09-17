# Changelog — tpt-av-visual-effects

All notable changes to the effects crate. The stack-wide changelog lives at
the repository root.

## [0.1.0] — initial release

### Added
- `Effect` trait: CPU reference implementation + WGSL GPU passes with a
  shared 96-byte uniform layout and optional 256-entry curve textures.
- `EffectRenderer`: cached pipelines per (shader, format), ping-pong scratch
  management for separable passes, fresh uniform buffers per pass.
- `build_effect` registry resolving timeline effect names to
  implementations.
- Built-ins: gaussian/box/motion blur, sharpen, color correct, levels, tone
  curves, chroma key (tolerance/softness/spill), noise generation, noise
  reduction, vignette.
- GPU-vs-CPU parity integration tests (skipped without an adapter).

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
