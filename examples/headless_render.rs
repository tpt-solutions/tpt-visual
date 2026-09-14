//! `headless_render` — renders a timeline (JSON) to an MJPEG AVI video file.
//!
//! Usage:
//! ```text
//! cargo run --example headless_render -- [session.json] [--frames 60] [--out out.avi]
//! ```
//!
//! Without a JSON file a demo timeline with a procedural source is rendered.
//! The JSON schema is the serde representation of `timeline::Session`.
//! This example always attaches procedural decoders (no media files
//! needed); combine with a JSON document that references them by
//! resolution, or extend `attach_decoders` below to use
//! `KinetixDecoder::from_bytes` for real MP4 sources.

#[path = "avi_writer.rs"]
mod avi_writer;

use avi_writer::AviWriter;
use tpt_av_visual_compositor::{ProceduralDecoder, TimelineRenderer};
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

fn demo_session() -> Session {
    let mut session = Session::new(
        "headless demo",
        FrameRate::film(),
        Resolution::new(320, 180).unwrap(),
    );
    let asset = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        "procedural://stripes",
        240,
        FrameRate::film(),
        Resolution::new(320, 180).unwrap(),
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let mut clip = Clip::new(session.allocate_clip_id(), asset.id, 0, 0, 120);
    clip.effects
        .push(timeline::EffectInstance::new("vignette").with_param("amount", 0.6));
    session.tracks[0].insert_clip(clip).unwrap();
    session
}

fn attach_decoders(session: &Session, renderer: &mut TimelineRenderer) {
    for asset in session.assets.values() {
        renderer.attach_asset(
            asset.clone(),
            Box::new(ProceduralDecoder::new(
                asset.frame_rate,
                asset.resolution,
            )),
        );
    }
}

fn parse_args() -> (Option<String>, u64, String) {
    let mut args = std::env::args().skip(1);
    let mut json_path = None;
    let mut frames = 60_u64;
    let mut out = "out.avi".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                frames = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
            }
            "--out" => {
                out = args.next().unwrap_or_else(|| "out.avi".into());
            }
            other => json_path = Some(other.to_string()),
        }
    }
    (json_path, frames, out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (json_path, frames, out_path) = parse_args();

    let session = match json_path {
        Some(path) => {
            let text = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Session>(&text)?
        }
        None => demo_session(),
    };
    let resolution = session.resolution;
    let fps = session.frame_rate.as_f32().round() as u32;

    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("no GPU adapter available; cannot render");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    attach_decoders(&session, &mut renderer);

    let file = std::fs::File::create(&out_path)?;
    let mut writer = AviWriter::new(std::io::BufWriter::new(file), resolution.width, resolution.height, fps.max(1));

    // Render into an offscreen texture, read back per frame, encode.
    let gpu = renderer.compositor_mut().gpu();
    let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless_render target"),
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

    const BPR: u32 = 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("headless_render readback"),
        size: u64::from(BPR * resolution.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    for index in 0..frames {
        renderer.render_frame(&view)?;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
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
    eprintln!("wrote {out_path} ({bytes} bytes, {frames} frames)");
    Ok(())
}

