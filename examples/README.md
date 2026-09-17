# Examples

All examples live in the facade crate and need a GPU. Invoke via the cargo
aliases configured in `.cargo/config.toml`, the `justfile`, or plain cargo.

| Example | What it shows | Run |
| :--- | :--- | :--- |
| `compositor_demo` | Three layers: transforms, Screen/Multiply blends, keyframed fade, vignette — exported to MJPEG AVI. | `cargo demo -- --frames 90 --out demo.avi` |
| `headless_render` | Timeline JSON document → MJPEG AVI (schema: [docs/session-json.md](../docs/session-json.md), sample: [examples/session.json](session.json)). | `cargo render examples/session.json out.avi 120` |
| `simple_player` | Plays a video file (`.mp4` via tpt-kinetix) or the procedural pattern in a window. | `cargo run --release -p tpt-av-visual --example simple_player -- clip.mp4` |

Equivalent `just` recipes: `just demo`, `just render <json> <out> <frames>`,
`just player <file>`.

Note: `headless_render` assets ending in `.mp4`/`.mov` decode through
`tpt-kinetix`; `procedural://` paths render the animated test pattern so the
examples work without media files.
