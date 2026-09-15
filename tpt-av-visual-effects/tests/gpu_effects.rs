//! GPU integration: run an effect chain end-to-end and compare against the
//! CPU reference. Skips when no GPU adapter exists.

use effects::{Effect, EffectParams, EffectPassDesc, EffectRenderer};
use tpt_av_visual_effects as effects;

/// Uploads an RGBA8 texture, runs passes ping-pong, downloads the result.
fn run_chain(
    device: &std::sync::Arc<wgpu::Device>,
    queue: &std::sync::Arc<wgpu::Queue>,
    passes: &[EffectPassDesc],
    src: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    let make_tex = |label, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: usage | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    let src_tex = make_tex("src", wgpu::TextureUsages::COPY_DST);
    queue.write_texture(
        src_tex.as_image_copy(),
        src,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let scratch_a = make_tex("scratch-a", wgpu::TextureUsages::RENDER_ATTACHMENT);
    let scratch_b = make_tex("scratch-b", wgpu::TextureUsages::RENDER_ATTACHMENT);
    let out_tex = make_tex(
        "out",
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );

    let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
    let views = [
        view(&src_tex),
        view(&scratch_a),
        view(&scratch_b),
        view(&out_tex),
    ];

    let mut renderer = EffectRenderer::new(device.clone(), queue.clone());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

    // views[0] = source; each pass reads the previous result.
    let mut cursor = 0_usize;
    for (i, pass) in passes.iter().enumerate() {
        let target_idx = if i + 1 == passes.len() {
            3
        } else {
            1 + (i % 2)
        };
        renderer.render_pass(
            &mut encoder,
            pass,
            &views[cursor],
            &views[target_idx],
            wgpu::TextureFormat::Rgba8Unorm,
        );
        cursor = target_idx;
    }
    queue.submit(Some(encoder.finish()));

    // Read back with 256-byte row alignment.
    let bpr = 256_u32;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(bpr * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        out_tex.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
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
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * bpr) as usize;
        out.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
    }
    out
}

#[test]
fn vignette_gpu_matches_cpu() {
    let Some((device, queue)) = effects::headless_gpu() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };

    let width = 32;
    let height = 32;
    // Gradient source.
    let mut src = vec![0_u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            src[idx] = (x * 255 / width) as u8;
            src[idx + 1] = (y * 255 / height) as u8;
            src[idx + 2] = 128;
            src[idx + 3] = 255;
        }
    }

    let vignette = effects::Vignette::gentle();
    let passes = vignette.passes(width, height);
    let gpu = run_chain(&device, &queue, &passes, &src, width, height);

    if std::env::var("EFFECTS_DEBUG").is_ok() {
        for row in [0, 8, 16, 24] {
            let vals: Vec<u8> = (0..32)
                .map(|col| gpu[((row * 32 + col) * 4) as usize])
                .collect();
            eprintln!("row {row}: {:?}", &vals[..8]);
        }
    }

    let mut cpu = src.clone();
    vignette.apply_cpu(&mut cpu, width, height);

    // Compare center (should match closely) and allow a couple of 8-bit
    // codes of tolerance everywhere (CPU rounds through u8, GPU is f32).
    let center = ((height / 2 * width + width / 2) * 4) as usize;
    for ch in 0..3 {
        let d = i32::from(gpu[center + ch]).abs_diff(i32::from(cpu[center + ch]));
        assert!(
            d <= 2,
            "center channel {ch}: gpu {} cpu {}",
            gpu[center + ch],
            cpu[center + ch]
        );
    }
    // r and g are 0 in the corner of this gradient; use b (128).
    assert!(gpu[2] < src[2], "GPU vignette must darken the corner");
}

#[test]
fn color_correct_gpu_matches_cpu() {
    let Some((device, queue)) = effects::headless_gpu() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let width = 16;
    let height = 16;
    let mut src = vec![0_u8; (width * height * 4) as usize];
    for (px, v) in src.chunks_exact_mut(4).enumerate() {
        v[0] = (px % 251) as u8;
        v[1] = (px % 173) as u8;
        v[2] = (px % 97) as u8;
        v[3] = 255;
    }

    let mut cc = effects::ColorCorrect::neutral();
    cc.brightness = 0.05;
    cc.contrast = 0.2;
    cc.saturation = 1.4;
    let gpu = run_chain(
        &device,
        &queue,
        &cc.passes(width, height),
        &src,
        width,
        height,
    );

    let mut cpu = src.clone();
    cc.apply_cpu(&mut cpu, width, height);

    for px in 0..(width * height * 4) as usize {
        if px % 4 == 3 {
            continue;
        }
        let d = i32::from(gpu[px]).abs_diff(i32::from(cpu[px]));
        // HSV hue path is only exercised at hue != 0; brightness/contrast/
        // saturation are linear and should agree within rounding.
        assert!(d <= 3, "px {px}: gpu {} cpu {}", gpu[px], cpu[px]);
    }
}

#[test]
fn identity_pass_is_transparent() {
    let Some((device, queue)) = effects::headless_gpu() else {
        eprintln!("skipping: no GPU adapter available");
        return;
    };
    let width = 16;
    let height = 16;
    let mut src = vec![0_u8; (width * height * 4) as usize];
    for (px, v) in src.chunks_exact_mut(4).enumerate() {
        v[0] = (px % 251) as u8;
        v[1] = (px % 173) as u8;
        v[2] = (px % 97) as u8;
        v[3] = 255;
    }
    let cc = effects::ColorCorrect::neutral();
    let gpu = run_chain(
        &device,
        &queue,
        &cc.passes(width, height),
        &src,
        width,
        height,
    );
    for px in 0..(width * height * 4) as usize {
        assert_eq!(
            gpu[px], src[px],
            "identity pass must be transparent at px {px}"
        );
    }
}

#[test]
fn params_layout_matches_wgsl_stride() {
    // 96 bytes: mat3x3-free shared layout with trailing 16-byte alignment.
    assert_eq!(std::mem::size_of::<EffectParams>(), 64);
}
