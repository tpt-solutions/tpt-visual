# Changelog — tpt-av-visual

All notable changes to the facade crate. The stack-wide changelog lives at
the repository root.

## [0.1.0] — initial release

### Added
- Re-exports of the full stack (`utils`, `timeline`, `compositor`, `color`,
  `effects`) with a curated `prelude`.
- `SessionBuilder` / `ClipSpec` — fluent, validated session construction
  with fades, effects, keyframes, and no manual id bookkeeping.
- `probe_gpu()` / `GpuInfo` — adapter/backend/driver diagnostics.
- `default_decoder` — MP4(H.264)/procedural decoder selection.
- `Error` / `Result` unifying the stack's error types.

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
