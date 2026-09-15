//! `compositor_demo` — composites multiple video layers: three procedural
//! sources with different transforms, blend modes, opacities, and effects,
//! rendered headlessly to an MJPEG AVI.
//!
//! Usage:
//! ```text
//! cargo demo -- --frames 90 --out demo.avi          # via the cargo alias
//! cargo run --release -p tpt-av-visual --example compositor_demo -- --frames 90
//! ```

use tpt_av_visual::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut frames = 90_u64;
    let mut out = "demo.avi".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => frames = args.next().and_then(|v| v.parse().ok()).unwrap_or(90),
            "--out" => out = args.next().unwrap_or_else(|| "demo.avi".into()),
            _ => {}
        }
    }

    let resolution = Resolution::new(480, 270).unwrap();
    let mut builder = SessionBuilder::new("compositor demo", FrameRate::film(), resolution)
        .add_video_asset("procedural://base") // asset 0
        .add_video_asset("procedural://pip") // asset 1
        .add_video_asset("procedural://tint"); // asset 2

    // Base layer: full-frame procedural stripes.
    builder = builder.add_clip(ClipSpec::new(0, 0, 0).duration(frames));

    // Picture-in-picture layer, scaled down, screen blended.
    builder = builder.add_track("PiP").add_clip(
        ClipSpec::new(1, 1, 0)
            .duration(frames)
            .transform(Transform {
                position: (80.0, 50.0),
                scale: (0.35, 0.35),
                rotation: 8.0,
                anchor: (0.5, 0.5),
            })
            .blend_mode(BlendMode::Screen)
            .opacity(0.9),
    );

    // Rotating tint layer, multiply blended, fading in via a keyframed
    // opacity animation (bezier ease). Note track index 1 = "Tint", added
    // right below.
    let mut fade = KeyframeTrack::new("opacity", InterpolationMethod::Bezier);
    fade.upsert_keyframe(Keyframe::bezier(0, 0.0, 0.25, 0.1, 0.25, 1.0));
    fade.upsert_keyframe(Keyframe::at(30, 0.55));
    builder = builder.add_track("Tint").add_clip(
        ClipSpec::new(2, 2, 0)
            .duration(frames)
            .transform(Transform {
                position: (0.0, 0.0),
                scale: (1.4, 1.4),
                rotation: -12.0,
                anchor: (0.5, 0.5),
            })
            .blend_mode(BlendMode::Multiply)
            .opacity(0.0)
            .effect("vignette", [("amount", 0.7)])
            .keyframes(fade),
    );

    let session = builder.build()?;

    // Renderer + default (procedural) decoders.
    let mut renderer = match TimelineRenderer::headless(session) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("no GPU adapter available; cannot run the demo");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    renderer.attach_default_decoders()?;

    let bytes = renderer.render_frames_to_avi(&out, frames, 90)?;
    eprintln!("wrote {out} ({bytes} bytes, {frames} frames)");
    Ok(())
}
