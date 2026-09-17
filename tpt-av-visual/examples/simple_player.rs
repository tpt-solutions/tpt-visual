//! `simple_player` — plays a single video source in a window.
//!
//! Usage:
//! ```text
//! cargo run --release -p tpt-av-visual --example simple_player -- path/to/video.mp4
//! cargo run --release -p tpt-av-visual --example simple_player    # procedural source
//! ```
//!
//! The window surface's BGRA format triggers the compositor's final-blit
//! R/B swap automatically.

use tpt_av_visual::prelude::*;
use tpt_av_visual::timeline::AssetId;

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

    // Window + surface.
    let event_loop = winit::event_loop::EventLoopBuilder::new().build()?;
    let window = winit::window::WindowBuilder::new()
        .with_title("tpt-visual simple player")
        .with_inner_size(winit::dpi::LogicalSize::new(960.0, 540.0))
        .build(&event_loop)?;

    let instance = renderer.compositor_mut().gpu().instance().clone();
    let surface = unsafe {
        // winit 0.29 + raw window handle on desktop.
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window)?)
    }?;
    let surface_format = {
        let caps = surface.get_capabilities(renderer.compositor_mut().gpu().adapter());
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        surface.configure(
            renderer.compositor_mut().gpu().device(),
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: window.inner_size().width.max(1),
                height: window.inner_size().height.max(1),
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );
        format
    };

    let start = std::time::Instant::now();
    let mut last_presented = u64::MAX;
    event_loop
        .run(move |event, elwt| {
            if let winit::event::Event::WindowEvent {
                event: winit::event::WindowEvent::RedrawRequested,
                ..
            } = event
            {
                let frame = (start.elapsed().as_secs_f64() * 24.0) as u64;
                if frame != last_presented {
                    renderer.seek(frame);
                    match surface.get_current_texture() {
                        Ok(frame_texture) => {
                            let surface_view = frame_texture
                                .texture
                                .create_view(&wgpu::TextureViewDescriptor::default());
                            match renderer.render_frame_into(&surface_view, surface_format) {
                                Ok(()) => {
                                    last_presented = frame;
                                    frame_texture.present();
                                }
                                Err(e) => eprintln!("render paused: {e}"),
                            }
                        }
                        Err(e) => eprintln!("surface error: {e}"),
                    }
                }
                window.request_redraw();
            }
            if let winit::event::Event::WindowEvent {
                event: winit::event::WindowEvent::CloseRequested,
                ..
            } = event
            {
                elwt.exit();
            }
        })
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    Ok(())
}
