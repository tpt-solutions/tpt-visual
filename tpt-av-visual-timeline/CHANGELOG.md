# Changelog — tpt-av-visual-timeline

All notable changes to the edit-model crate. The stack-wide changelog lives
at the repository root.

## [0.1.0] — initial release

### Added
- `Session` / `Track` / `Clip` / `VideoAsset` non-destructive edit model.
- `Transform` (position/scale/rotation/anchor) with canvas-affine mapping.
- `KeyframeTrack` animation: linear, Catmull-Rom, and bezier interpolation
  with CSS-style control points.
- Eased `Clip::fade_in` / `Clip::fade_out` envelope helpers (`FadeCurve`).
- 13 `BlendMode`s and `EffectInstance` parameter bags.
- Edit operations (`InsertClip`, `DeleteClip`, `MoveClip`, `SplitClip`) as
  validated `Reversible` commands with undo/redo `History`.
- Complete serde support plus JSON document helpers (`from_json_path`,
  `to_json_path`, ...).
- Id cursors preserved across serialization so post-load edits never collide.

[Unreleased]: https://github.com/tpt-solutions/tpt-visual/compare/v0.1.0...HEAD
