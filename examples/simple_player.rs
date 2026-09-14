//! `simple_player` — plays a single video source in a window.
//!
//! Usage:
//! ```text
//! cargo run --example simple_player                       # procedural source
//! cargo run --example simple_player -- path/to/video.mp4  # H.264 via kinetix
//! ```
//!
//! The window surface's BGRA format triggers the compositor's final-blit
//! R/B swap automatically.

use tpt_av_visual_compositor::{ProceduralDecoder, TimelineRenderer};
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let media_path = std::env::args().nth(1);

    let resolution = Resolution::new(960, 540).unwrap();
    let mut session = Session::new("simple player", FrameRate::film(), resolution);
    let asset = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        media_path.clone().unwrap_or_else(|| "procedural://player".into()),
        24_000,
        FrameRate::film(),
        resolution,
        PixelFormat::Yuv420p,
        "Rec709",
    ));
    let clip = Clip::new(session.allocate_clip_id(), asset.id, 0, 0, 24_000);
    session
        .tracks[0]
        .insert_clip(clip)?;

    let mut renderer = match TimelineRenderer::headless(session.clone()) {
        Ok(Some(renderer)) => renderer,
        Ok(None) => {
            eprintln!("no GPU adapter available");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };
    attach_decoder(&mut renderer, &asset, media_path.as_deref());

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
    let (surface_format, _surface_config_unused) = {
        let caps = surface.get_capabilities(renderer.compositor_mut().gpu().adapter());
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: window.inner_size().width.max(1),
            height: window.inner_size().height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(renderer.compositor_mut().gpu().device(), &config);
        (format, config)
    };

    let start = std::time::Instant::now();
    let mut last_frame_rendered = u64::MAX;
    event_loop.run(move |event, elwt| {
        if let winit::event::Event::WindowEvent {
            event: winit::event::WindowEvent::RedrawRequested,
            ..
        } = event
        {
            let elapsed = start.elapsed().as_secs_f64();
            let frame = (elapsed * 24.0) as u64;
            if frame != last_frame_rendered {
                renderer.seek(frame);
                match surface.get_current_texture() {
                    Ok(frame_texture) => {
                        let surface_view = frame_texture
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        match renderer.render_frame_into(&surface_view, surface_format) {
                            Ok(()) => {
                                last_frame_rendered = frame;
                                frame_texture.present();
                            }
                            Err(e) => {
                                // Beyond end of stream: hold the last frame.
                                eprintln!("render paused: {e}");
                            }
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

#[cfg(feature = "kinetix")]
fn attach_decoder(
    renderer: &mut TimelineRenderer,
    asset: &timeline::VideoAsset,
    media_path: Option<&str>,
) {
    match media_path {
        Some(path) => match std::fs::read(path) {
            Ok(data) => {
                renderer.attach_asset(
                    asset.clone(),
                    Box::new(tpt_av_visual_compositor::KinetixDecoder::from_bytes(
                        data,
                        asset.frame_rate,
                        asset.resolution,
                    )),
                );
            }
            Err(e) => {
                eprintln!("cannot read {path}: {e}; falling back to procedural source");
                fallback(renderer, asset);
            }
        },
        None => fallback(renderer, asset),
    }
}

#[cfg(feature = "kinetix")]
fn fallback(renderer: &mut TimelineRenderer, asset: &timeline::VideoAsset) {
    renderer.attach_asset(
        asset.clone(),
        Box::new(ProceduralDecoder::new(asset.frame_rate, asset.resolution)),
    );
}

#[cfg(not(feature = "kinetix"))]
fn attach_decoder(
    renderer: &mut TimelineRenderer,
    asset: &timeline::VideoAsset,
    media_path: Option<&str>,
) {
    if media_path.is_some() {
        eprintln!("built without the `kinetix` feature; using procedural source");
    }
    renderer.attach_asset(
        asset.clone(),
        Box::new(ProceduralDecoder::new(asset.frame_rate, asset.resolution)),
    );
}
