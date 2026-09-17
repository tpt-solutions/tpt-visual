//! `headless_render` — renders a timeline (JSON) to an MJPEG AVI video file.
//!
//! Usage:
//! ```text
//! cargo render session.json out.avi 240          # via the cargo alias
//! cargo run --release -p tpt-av-visual --example headless_render -- \
//!     session.json --out out.avi --frames 240
//! ```
//!
//! The JSON schema is the serde representation of `timeline::Session`
//! (`Session::to_json_path` writes it). Assets whose path ends in `.mp4`/
//! `.mov` decode via `tpt-kinetix` (default `kinetix` feature); everything
//! else renders the procedural test pattern.

use tpt_av_visual::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut json_path: Option<String> = None;
    let mut frames = 60_u64;
    let mut out = "out.avi".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => frames = args.next().and_then(|v| v.parse().ok()).unwrap_or(60),
            "--out" => out = args.next().unwrap_or_else(|| "out.avi".into()),
            "--help" | "-h" => {
                println!("usage: headless_render <session.json> [--frames N] [--out out.avi]");
                return Ok(());
            }
            other => json_path = Some(other.to_string()),
        }
    }
    let Some(json_path) = json_path else {
        eprintln!("usage: headless_render <session.json> [--frames N] [--out out.avi]");
        eprintln!("(without a JSON path this example does nothing)");
        return Ok(());
    };

    let session = Session::from_json_path(&json_path)?;

    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("no GPU adapter available; cannot render");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    renderer.attach_default_decoders()?;

    let bytes = renderer.render_frames_to_avi(&out, frames, 90)?;
    eprintln!("wrote {out} ({bytes} bytes, {frames} frames)");
    Ok(())
}
