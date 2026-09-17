# tpt-av-visual-timeline

The pure, non-destructive video-edit data model of the TPT AV visual stack.
It describes *what* plays, *when*, and *how* — it never touches media, has no
GPU code, and is fully serde-serializable to JSON.

Part of [tpt-visual](https://github.com/tpt-solutions/tpt-visual). Consumed by
[`tpt-av-visual-compositor`](../tpt-av-visual-compositor). Dual-licensed MIT OR
Apache-2.0.

## Features

- **Session model** — `Session` → `Track`s (bottom→top z-order) → `Clip`s
  referencing immutable `VideoAsset`s; rational frame rates and session-wide
  resolution.
- **Non-destructive clips** — `start_frame` / `source_offset` /
  `duration_frames` trim math, spatial `Transform`
  (position/scale/rotation/anchor), opacity, 13 blend modes, and
  `EffectInstance` parameter bags (decoupled from effect implementations).
- **Keyframe animation** — scalar property tracks (`transform.position.y`,
  `opacity`, `effects.0.radius`, ...) with linear, Catmull-Rom, and
  bezier interpolation (CSS-style control points), plus eased
  **fade-in/fade-out** helpers.
- **Edit operations** — insert/delete/move/split as validated `Reversible`
  commands; full **undo/redo** `History` with bounded depth.
- **JSON documents** — `Session::from_json_path` / `to_json_path` round-trip
  the entire edit, including id cursors so post-load edits never collide.

## Install

```toml
[dependencies]
tpt-av-visual-timeline = "0.1"
```

## Example

```rust
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{
    AssetId, BlendMode, Clip, History, Session,
};
use tpt_av_visual_utils::{FrameRate, Resolution};

let mut session = Session::new("Demo", FrameRate::film(), Resolution::full_hd());
let asset = session.register_asset(timeline::VideoAsset::new(
    AssetId(0), "media/interview.mp4", 480,
    FrameRate::film(), Resolution::full_hd(),
    tpt_av_visual_utils::PixelFormat::Yuv420p, "Rec709",
));

let mut clip = Clip::new(session.allocate_clip_id(), asset.id, 24, 12, 96);
clip.blend_mode = BlendMode::Screen;
session.tracks[0].insert_clip(clip)?;

// Split at frame 72 — undo/redo included.
let mut history = History::default();
history.split_clip(&mut session, session.tracks[0].clips[0].id, 72)?;
assert!(history.undo(&mut session)?);

// Serialize the whole edit.
let json = session.to_json_string(true)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Testing

```sh
cargo test -p tpt-av-visual-timeline
```

Covers clip splitting, keyframe interpolation (linear/Catmull-Rom/bezier),
edit undo/redo cycles, overlap rejection, and serde round-trips.

## License

Dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](../LICENSE-MIT) and
[LICENSE-APACHE](../LICENSE-APACHE) at the repository root.
