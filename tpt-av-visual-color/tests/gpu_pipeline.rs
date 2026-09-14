//! GPU integration test: the fused color pass must agree with the CPU
//! reference implementation. Skips silently when no GPU adapter exists.

use tpt_av_visual_color as color;
use color::{ColorPipeline, ColorSpace, TransferFunction, ToneMapper};

#[test]
fn gpu_pipeline_matches_cpu_reference() {
    let Some((device, queue)) = color::headless_device() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let pipeline = ColorPipeline::new(
        ColorSpace::Rec2020,
        TransferFunction::Pq,
        ColorSpace::Srgb,
        TransferFunction::Srgb,
    )
    .with_tone_mapper(ToneMapper::AcesFilmic)
    .with_input_linear_scale(1.0 / 0.0203);

    // A 2x2 source texture of PQ-encoded reference white.
    let code = TransferFunction::Pq.encode(0.0203);
    let code_px = (code.clamp(0.0, 1.0) * 255.0).round() as u8;
    let size = 2;
    let src_data = vec![code_px, code_px, code_px, 255, code_px, code_px, code_px, 255, code_px, code_px, code_px, 255, code_px, code_px, code_px, 255];

    let src_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("src"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        src_tex.as_image_copy(),
        &src_data,
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: None },
        wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
    );
    let src_view = src_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let dst_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dst"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let dst_view = dst_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    pipeline
        .apply(&device, &queue, &mut encoder, &src_view, &dst_view, wgpu::TextureFormat::Rgba8Unorm)
        .expect("gpu apply");
    queue.submit(Some(encoder.finish()));

    // Read back (COPY_BYTES_PER_ROW_ALIGNMENT is 256).
    const BYTES_PER_ROW: u32 = 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (BYTES_PER_ROW * size) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        dst_tex.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(BYTES_PER_ROW), rows_per_image: None },
        },
        wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));

    let (tx, rx) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        tx.send(result).unwrap();
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().expect("map");

    let data: Vec<u8> = slice.get_mapped_range().to_vec();
    drop(slice);

    // CPU reference: the same 8-bit code through apply_pixel.
    let cpu_in = f32::from(code_px) / 255.0;
    let cpu_out = pipeline.apply_pixel([cpu_in, cpu_in, cpu_in]);
    let expected_r = (cpu_out[0].clamp(0.0, 1.0) * 255.0).round() as i32;

    // The GPU pass (linear sampling, shader math) must land within a couple
    // of 8-bit codes of the CPU reference.
    for row in 0..size {
    for px_index in 0..size {
    let px = &data[((row as usize) * (BYTES_PER_ROW as usize)) + (px_index as usize) * 4..][..4];
        let diff = (i32::from(px[0]) - expected_r).abs();
        assert!(diff <= 2, "gpu {} vs cpu {expected_r} (px {:?})", px[0], px);
        assert_eq!(px[1], px[0], "neutral grey stays neutral");
        assert_eq!(px[3], 255);
    }
    }
}
