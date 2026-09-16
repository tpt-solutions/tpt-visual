//! Compositor benchmarks. These measure the GPU render path and therefore
//! require an adapter; without one the binary exits immediately (CI keeps
//! `cargo test`/`cargo bench` green on GPU-less machines).

use compositor::{ProceduralDecoder, TimelineRenderer};
use criterion::{criterion_group, criterion_main, Criterion};
use tpt_av_visual_compositor as compositor;
use tpt_av_visual_timeline as timeline;
use tpt_av_visual_timeline::{AssetId, Clip, Session};
use tpt_av_visual_utils::{FrameRate, PixelFormat, Resolution};

fn bench_session() -> (Session, timeline::VideoAsset) {
    let mut session = Session::new(
        "bench",
        FrameRate::film(),
        Resolution::new(640, 360).unwrap(),
    );
    let asset = session.register_asset(timeline::VideoAsset::new(
        AssetId(0),
        "procedural://bench",
        10_000,
        FrameRate::film(),
        Resolution::new(640, 360).unwrap(),
        PixelFormat::Rgba8,
        "Rec709",
    ));
    let mut clip = Clip::new(session.allocate_clip_id(), asset.id, 0, 0, 10_000);
    clip.effects
        .push(timeline::EffectInstance::new("vignette").with_param("amount", 0.5));
    session.tracks[0].insert_clip(clip).unwrap();
    (session, asset)
}

fn bench_composite(c: &mut Criterion) {
    let Some(mut renderer) = TimelineRenderer::headless(bench_session().0).expect("engine") else {
        eprintln!("no GPU adapter; skipping compositor benchmarks");
        return;
    };
    let (_, asset) = bench_session();
    renderer.attach_asset(
        asset,
        Box::new(ProceduralDecoder::new(
            FrameRate::film(),
            Resolution::new(640, 360).unwrap(),
        )),
    );
    let device = renderer.compositor_mut().gpu().device().clone();
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench target"),
        size: wgpu::Extent3d {
            width: 640,
            height: 360,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // Warm up pipeline caches.
    renderer.render_frame(&view).expect("warmup");

    let mut group = c.benchmark_group("compositor");
    group.throughput(criterion::Throughput::Elements(1));
    group.bench_function("composite_640x360_vignette", |b| {
        b.iter(|| renderer.render_frame(&view).expect("bench frame"))
    });
    group.finish();
}

criterion_group!(benches, bench_composite);
criterion_main!(benches);
