//! End-to-end render pipeline tests (GPU required; skipped without one).

use tpt_av_visual_compositor as compositor;
use compositor::{FrameDecoder, ProceduralDecoder, TimelineRenderer};
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution, VideoFrame};

fn test_session() -> (Session, timeline::VideoAsset) {
    let mut session = Session::new("render test", FrameRate::film(), Resolution::new(64, 64).unwrap());
    let asset = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        "procedural",
        120,
        FrameRate::film(),
        Resolution::new(64, 64).unwrap(),
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let clip = Clip::new(session.allocate_clip_id(), asset.id, 0, 0, 120);
    session.tracks[0].insert_clip(clip).unwrap();
    (session, asset)
}

fn read_back(
    device: &std::sync::Arc<wgpu::Device>,
    queue: &std::sync::Arc<wgpu::Queue>,
    view: &wgpu::TextureView,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let _ = view;
    const BPR: u32 = 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(BPR * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(BPR),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().expect("map");
    let mapped = slice.get_mapped_range();
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * BPR) as usize;
        out.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
    }
    out
}

#[test]
fn full_pipeline_renders_changing_frames() {
    let Some(gpu) = compositor::GpuContext::headless() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let (session, asset) = test_session();
    let mut renderer = TimelineRenderer::headless(session)
        .expect("renderer")
        .expect("GPU available");

    renderer.attach_asset(
        asset,
        Box::new(ProceduralDecoder::new(
            FrameRate::film(),
            Resolution::new(64, 64).unwrap(),
        )),
    );

    let device = gpu.device();
    let queue = gpu.queue();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // Frame 0.
    renderer.render_frame(&view).expect("frame 0");
    let out0 = read_back(device, queue, &view, &target, 64, 64);
    assert_eq!(renderer.playhead(), 1, "playhead advances");

    // Frame 1.
    renderer.render_frame(&view).expect("frame 1");
    let out1 = read_back(device, queue, &view, &target, 64, 64);

    // The procedural pattern animates, so consecutive frames must differ.
    let differing = out0
        .iter()
        .zip(&out1)
        .filter(|(a, b)| a.abs_diff(**b) > 2)
        .count();
    assert!(differing > 64, "frames must animate (differing pixels: {differing})");

    // The canvas must be non-transparent where the clip rendered.
    let alpha = out0.iter().skip(3).step_by(4).next().copied().unwrap_or(0);
    assert_eq!(alpha, 255, "clip covers the full canvas");
}

#[test]
fn yuv_frames_upload_through_gpu_conversion() {
    let Some(gpu) = compositor::GpuContext::headless() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    // Build a YUV420p grey frame (BT.709 limited): luma 235 + neutral
    // chroma 128 must convert to white.
    let mut data = vec![235_u8; 4 * 4];
    data.extend_from_slice(&[128_u8; 4 + 4]); // 2x2 chroma planes
    let frame = VideoFrame::new(4, 4, PixelFormat::Yuv420p, data, 0).unwrap();

    let device = gpu.device();
    let queue = gpu.queue();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let pipeline = {
        let mut compositor = compositor::Compositor::new(gpu.clone());
        compositor.yuv_pipeline().unwrap()
    };
    let texture = compositor::gpu::texture::GpuTexture::upload(
        device,
        queue,
        &mut encoder,
        &frame,
        &pipeline,
    )
    .unwrap();
    queue.submit(Some(encoder.finish()));

    let out = read_back(device, queue, texture.view(), texture.texture(), 4, 4);
    for px in out.chunks_exact(4) {
        assert!(px[0].abs_diff(255) <= 2, "white from luma 235: {px:?}");
        assert_eq!(px[0], px[1], "neutral chroma stays grey");
    }
}

#[test]
fn prefetch_threads_shut_down_cleanly() {
    let Some(gpu) = compositor::GpuContext::headless() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let (session, asset) = test_session();
    let mut renderer = TimelineRenderer::headless(session)
        .expect("renderer")
        .expect("GPU available");
    renderer.attach_asset(
        asset,
        Box::new(ProceduralDecoder::new(
            FrameRate::film(),
            Resolution::new(64, 64).unwrap(),
        )),
    );

    renderer.prefetch_around_playhead(8);
    let device = gpu.device();
    let queue = gpu.queue();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    // Rendering drains prefetched frames without deadlocking on the decoder.
    for _ in 0..4 {
        renderer.render_frame(&view).expect("prefetched frame");
    }
    // Dropping the renderer stops and joins the prefetch thread.
    drop(renderer);
}
