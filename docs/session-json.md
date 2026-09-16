# Session JSON schema

A `tpt-visual` timeline document is the serde JSON representation of
[`tpt_av_visual_timeline::Session`](../tpt-av-visual-timeline). The canonical
example in this repository is [`examples/session.json`](../examples/session.json);
render it with:

```sh
cargo render examples/session.json out.avi 120
# or
just render examples/session.json out.avi 120
```

`Session::to_json_path` writes exactly this format and
`Session::from_json_path` reads it, so a document saved by your application
round-trips through the CLI renderer unchanged.

## Top-level fields

| Field | Type | Notes |
| :--- | :--- | :--- |
| `id`, `name` | u64, string | Session identity. |
| `frame_rate` | `{num, den}` | Rational frame rate; `{"num":30000,"den":1001}` is NTSC. |
| `resolution` | `{width, height}` | Canvas size for the whole session. |
| `tracks` | array | Ordered bottom→top by z-index. |
| `assets` | map `id → asset` | Metadata only; media files are never modified. |
| `metadata` | object | Timecode start, working color space, description, tags. |
| `next_track_id` / `next_clip_id` / `next_asset_id` | u64 | Id allocation cursors — preserved on round-trip so new edits never collide with old ids. |

## Tracks and clips

Each track carries `clips` (sorted by `start_frame`), plus track-level
`opacity`, `blend_mode`, `hidden`, and `locked`. A clip references its asset
by `asset_id` and plays `duration_frames` starting at `start_frame`,
optionally trimmed by `source_offset`.

- `transform` — `position` (canvas px), `scale`, `rotation` (degrees
  clockwise), `anchor` (normalized 0–1 rotation center).
- `blend_mode` — one of `Normal`, `Multiply`, `Screen`, `Overlay`,
  `Darken`, `Lighten`, `HardLight`, `Difference`, `Exclusion`, `Hue`,
  `Saturation`, `Color`, `Luminosity`.
- `keyframes` — scalar animations; `property` paths include
  `transform.position.x/y`, `transform.scale.x/y`, `transform.rotation`,
  `opacity`, and effect parameters as `effects.<index>.<param>`.
  `interpolation` is `Linear`, `Cubic` (Catmull-Rom), or `Bezier` with
  per-keyframe `bezier: [x1, y1, x2, y2]` control points.
- `effects` — name + numeric parameters; names map onto the effect registry
  (`gaussian_blur`, `vignette`, `chroma_key`, `color_correct`, ...).

## Asset paths

Assets ending in `.mp4`/`.mov` decode via `tpt-kinetix` (the default
`kinetix` feature). The `procedural://` pseudo-protocol renders the animated
test pattern — useful for demos without media files. Unknown paths fall back
to the procedural source with a warning.
