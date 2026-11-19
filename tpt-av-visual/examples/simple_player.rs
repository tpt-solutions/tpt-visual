//! `simple_player` — plays a single video source in a window.
//!
//! Usage:
//! ```text
//! cargo run --release -p tpt-av-visual --example simple_player -- path/to/video.mp4
//! cargo run --release -p tpt-av-visual --example simple_player    # procedural source
//! ```
//!
//! The window is provided by `tpt-window`; the surface is created from its raw
//! handles. The surface is RGBA: a BGRA surface does not work yet (see the
//! compositor note in `todo.md`).

use std::time::{Duration, Instant};

use tpt_av_visual::prelude::*;
use tpt_av_visual::timeline::AssetId;
use tpt_window::{
    EventQueue, EventSink, LogicalSize, PollStatus, WindowBuilder, WindowEvent, DEFAULT_CAPACITY,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let media_path = std::env::args().nth(1);

    let resolution = Resolution::new(960, 540).unwrap();
    let mut session = Session::new("simple player", FrameRate::film(), resolution);
    let asset = session.register_asset(VideoAsset::new(
        AssetId(0),
        media_path
            .clone()
            .unwrap_or_else(|| "procedural://player".into()),
        24_000,
        FrameRate::film(),
        resolution,
        PixelFormat::Yuv420p,
        "Rec709",
    ));
    let clip = Clip::new(session.allocate_clip_id(), asset.id, 0, 0, 24_000);
    session.tracks[0].insert_clip(clip)?;

    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("no GPU adapter available");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    // `default_decoder` routes .mp4/.mov to tpt-kinetix (feature `kinetix`)
    // and everything else to the procedural pattern.
    let decoder = tpt_av_visual::default_decoder(&asset)?;
    renderer.attach_asset(asset, decoder);

    // Window first, then surface: locals drop in reverse order, so the surface
    // (which refers to the window's raw handles) drops before the window.
    let mut event_loop = tpt_window::new_event_loop()?;
    let window = WindowBuilder::new()
        .with_title("tpt-visual simple player")
        .with_inner_size(LogicalSize::new(960.0, 540.0))
        .build(&event_loop)?;

    let instance = renderer.compositor_mut().gpu().instance().clone();
    let device = renderer.compositor_mut().gpu().device().clone();
    let gpu_queue = renderer.compositor_mut().gpu().queue().clone();
    let adapter = renderer.compositor_mut().gpu().adapter().clone();
    let surface = unsafe {
        // SAFETY: `window` outlives `surface` (declared earlier, dropped later).
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(window.raw_display_handle()?),
            raw_window_handle: window.raw_window_handle()?,
        })
    }?;

    let caps = surface.get_capabilities(&adapter);
    // Prefer RGBA. The compositor's intermediate passes render to RGBA textures,
    // so a BGRA surface currently fails validation (see tpt-visual todo.md).
    // Do not take the first match: wgpu 30 lists formats in a different order.
    let surface_format = [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Bgra8Unorm,
    ]
    .into_iter()
    .find(|want| caps.formats.contains(want))
    .unwrap_or(wgpu::TextureFormat::Rgba8Unorm);
    let size = window.size();
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
        color_space: wgpu::SurfaceColorSpace::Auto,
    };
    surface.configure(&device, &config);

    let start = Instant::now();
    let mut last_presented = u64::MAX;
    let mut queue = EventQueue::with_capacity(DEFAULT_CAPACITY);
    'main: loop {
        // Wake at least every 4 ms so playback keeps time without OS events.
        let status = event_loop.step(&mut EventSink::new(&mut queue), Duration::from_millis(4));
        for ev in queue.drain() {
            match ev.event {
                WindowEvent::CloseRequested => break 'main,
                WindowEvent::Resized(size) => {
                    config.width = size.width.max(1);
                    config.height = size.height.max(1);
                    surface.configure(&device, &config);
                    // Repaint the current frame at the new size.
                    last_presented = u64::MAX;
                }
                _ => {}
            }
        }
        if status == PollStatus::Exit {
            break;
        }

        let frame = (start.elapsed().as_secs_f64() * 24.0) as u64;
        if frame != last_presented {
            renderer.seek(frame);
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame_texture)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame_texture) => {
                    let surface_view = frame_texture
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    match renderer.render_frame_into(&surface_view, surface_format) {
                        Ok(()) => {
                            last_presented = frame;
                            gpu_queue.present(frame_texture);
                        }
                        Err(e) => eprintln!("render paused: {e}"),
                    }
                }
                other => eprintln!("surface unavailable: {other:?}"),
            }
        }
    }
    Ok(())
}
