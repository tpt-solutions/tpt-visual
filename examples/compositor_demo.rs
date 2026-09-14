//! `compositor_demo` — composites multiple video layers: three procedural
//! sources with different transforms, blend modes, opacities, and effects,
//! rendered headlessly to an MJPEG AVI.
//!
//! Usage:
//! ```text
//! cargo run --example compositor_demo -- [--frames 90] [--out demo.avi]
//! ```

#[path = "avi_writer.rs"]
mod avi_writer;

use avi_writer::AviWriter;
use tpt_av_visual_compositor::{ProceduralDecoder, TimelineRenderer};
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{
    AssetId, BlendMode, Clip, EffectInstance, Session, Transform, VideoAsset,
};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

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
    let mut session = Session::new("compositor demo", FrameRate::film(), resolution);

    // Base layer: full-frame procedural stripes.
    let base = session.register_asset(VideoAsset::new(
        AssetId(0),
        "procedural://base",
        240,
        FrameRate::film(),
        resolution,
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let base_clip = Clip::new(session.allocate_clip_id(), base.id, 0, 0, frames);
    session
        .tracks[0]
        .insert_clip(base_clip)?;

    // Picture-in-picture layer, slightly scaled, screen blended.
    let pip = session.register_asset(VideoAsset::new(
        AssetId(0),
        "procedural://pip",
        240,
        FrameRate::film(),
        resolution,
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let mut pip_clip = Clip::new(session.allocate_clip_id(), pip.id, 0, 0, frames);
    pip_clip.transform = Transform {
        position: (80.0, 50.0),
        scale: (0.35, 0.35),
        rotation: 8.0,
        anchor: (0.5, 0.5),
    };
    pip_clip.blend_mode = BlendMode::Screen;
    pip_clip.opacity = 0.9;
    let overlay_track = session.add_track("PiP");
    session
        .track_checked_mut(overlay_track)
        .expect("track")
        .insert_clip(pip_clip)?;

    // Rotating vignette-tinted layer, multiply blended, fading in via a
    // keyframed opacity animation.
    let tint = session.register_asset(VideoAsset::new(
        AssetId(0),
        "procedural://tint",
        240,
        FrameRate::film(),
        resolution,
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let mut tint_clip = Clip::new(session.allocate_clip_id(), tint.id, 0, 0, frames);
    tint_clip.transform = Transform {
        position: (0.0, 0.0),
        scale: (1.4, 1.4),
        rotation: -12.0,
        anchor: (0.5, 0.5),
    };
    tint_clip.blend_mode = BlendMode::Multiply;
    tint_clip.opacity = 0.0;
    tint_clip
        .effects
        .push(EffectInstance::new("vignette").with_param("amount", 0.7));
    let mut fade = timeline::KeyframeTrack::new("opacity", timeline::InterpolationMethod::Bezier);
    fade.upsert_keyframe(timeline::Keyframe::bezier(0, 0.0, 0.25, 0.1, 0.25, 1.0));
    fade.upsert_keyframe(timeline::Keyframe::at(30, 0.55));
    tint_clip.keyframes.push(fade);
    let tint_track = session.add_track("Tint");
    session
        .track_checked_mut(tint_track)
        .expect("track")
        .insert_clip(tint_clip)?;

    // Renderer + procedural decoders.
    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("no GPU adapter available; cannot run the demo");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    for asset in session.assets.values() {
        renderer.attach_asset(
            asset.clone(),
            Box::new(ProceduralDecoder::new(asset.frame_rate, asset.resolution)),
        );
    }

    let (device, queue) = {
        let gpu = renderer.compositor_mut().gpu();
        (gpu.device().clone(), gpu.queue().clone())
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("compositor_demo target"),
        size: wgpu::Extent3d {
            width: resolution.width,
            height: resolution.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let file = std::fs::File::create(&out)?;
    let mut writer = AviWriter::new(
        std::io::BufWriter::new(file),
        resolution.width,
        resolution.height,
        24,
    );

    const BPR: u32 = 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compositor_demo readback"),
        size: u64::from(BPR * resolution.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    for index in 0..frames {
        renderer.render_frame(&view)?;

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(BPR),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: resolution.width,
                height: resolution.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().expect("map");

        let mapped = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity((resolution.width * resolution.height * 4) as usize);
        for row in 0..resolution.height {
            let start = (row * BPR) as usize;
            rgba.extend_from_slice(&mapped[start..start + (resolution.width * 4) as usize]);
        }
        drop(mapped);
        readback.unmap();

        writer.add_rgba(&rgba, resolution.width, resolution.height, 90)?;
        eprintln!("\rrendered frame {}/{}", index + 1, frames);
    }

    let bytes = writer.finish()?;
    eprintln!("wrote {out} ({bytes} bytes)");
    Ok(())
}
