//! The `Effect` trait and shared GPU plumbing.
//!
//! Every effect provides:
//! - a **CPU reference implementation** ([`Effect::apply_cpu`]), used by
//!   tests, validation, and software fallbacks, and
//! - one or more **GPU passes** ([`Effect::passes`]) driving WGSL shaders in
//!   `gpu_shaders/` through a shared uniform layout ([`EffectParams`]).
//!
//! [`EffectRenderer`] executes effect passes on the GPU. wgpu 0.19 resource
//! handles are not `Clone`, so the renderer owns its `Device`/`Queue` behind
//! `Arc`s; the compositor shares the same `Arc`s with it.

use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Errors surfaced by the effects crate.
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    /// An unknown effect name was requested from the registry.
    #[error("unknown effect: {0}")]
    UnknownEffect(String),
    /// A GPU operation failed.
    #[error("GPU error: {0}")]
    Gpu(String),
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, EffectError>;

/// A video effect: CPU reference implementation plus GPU pass list.
pub trait Effect: Send + Sync {
    /// Registered effect name (matches `tpt-av-visual-timeline`
    /// `EffectInstance::effect_name`).
    fn name(&self) -> &'static str;

    /// Applies the effect to a packed RGBA8 buffer (CPU reference).
    fn apply_cpu(&self, rgba: &mut [u8], width: u32, height: u32);

    /// The GPU pass list for this effect at the given target size.
    fn passes(&self, width: u32, height: u32) -> Vec<EffectPassDesc>;
}

/// Shared uniform layout for every effect shader (96-byte WGSL-safe stride).
///
/// The semantics of `p0`/`p1`/`mode` are documented per shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EffectParams {
    /// Effect-specific parameter block 0.
    pub p0: [f32; 4],
    /// Effect-specific parameter block 1.
    pub p1: [f32; 4],
    /// `1.0 / width`, `1.0 / height` of the pass target.
    pub texel: [f32; 2],
    /// Shader sub-mode selector.
    pub mode: u32,
    /// Hash seed (noise effects); otherwise unused.
    pub seed: f32,
    /// Reserved so the Rust stride matches WGSL's 16-byte-aligned struct
    /// size (64 bytes).
    pub pad: [f32; 4],
}

impl EffectParams {
    /// Default (all-zero) parameter block for a `width x height` target.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        EffectParams {
            p0: [0.0; 4],
            p1: [0.0; 4],
            texel: [1.0 / width as f32, 1.0 / height as f32],
            mode: 0,
            seed: 0.0,
            pad: [0.0; 4],
        }
    }
}

/// One GPU draw for an effect: a shader, its uniform block, and an
/// optional 256x1 RGBA8 curve LUT (`color_correct.wgsl` curve mode).
#[derive(Clone)]
pub struct EffectPassDesc {
    /// WGSL source (a `gpu_shaders/*.wgsl` file embedded with `include_str!`).
    pub shader_source: &'static str,
    /// Uniform data for this pass.
    pub params: EffectParams,
    /// Optional per-channel 256-entry curve LUT (curve mode in
    /// `color_correct.wgsl`).
    pub curve_lut: Option<Vec<[u8; 4]>>,
}

/// Device-resident effect executor. Compiles (and caches) one render
/// pipeline per (shader, target format) pair; effect passes are stateless,
/// so pipelines are reused across frames.
pub struct EffectRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    curve_sampler: wgpu::Sampler,
    neutral_lut: wgpu::TextureView,
    pipelines: HashMap<(&'static str, wgpu::TextureFormat), wgpu::RenderPipeline>,
}

impl EffectRenderer {
    /// Creates a renderer owning the GPU context.
    #[must_use]
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tpt-visual: effect bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Curve LUT (color_correct.wgsl curve mode). Layout entries
                // a shader never declares are legal and simply unused.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tpt-visual: effect sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..wgpu::SamplerDescriptor::default()
        });
        let curve_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tpt-visual: effect curve sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..wgpu::SamplerDescriptor::default()
        });
        let neutral_lut_view = {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tpt-visual: effect neutral LUT"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &[255_u8, 255, 255, 255],
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            texture.create_view(&wgpu::TextureViewDescriptor::default())
        };
        EffectRenderer {
            device,
            queue,
            bind_layout,
            sampler,
            curve_sampler,
            neutral_lut: neutral_lut_view,
            pipelines: HashMap::new(),
        }
    }

    /// The GPU queue (shared ownership with the compositor).
    #[must_use]
    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }

    /// The GPU device (shared ownership with the compositor).
    #[must_use]
    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }

    fn pipeline_for(
        &mut self,
        shader_source: &'static str,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        if !self.pipelines.contains_key(&(shader_source, format)) {
            let shader = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("tpt-visual: effect shader"),
                    source: wgpu::ShaderSource::Wgsl(shader_source.into()),
                });
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("tpt-visual: effect pipeline layout"),
                    bind_group_layouts: &[&self.bind_layout],
                    push_constant_ranges: &[],
                });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("tpt-visual: effect pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: "fs_main",
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                });
            self.pipelines.insert((shader_source, format), pipeline);
        }
        &self.pipelines[&(shader_source, format)]
    }

    /// Records a single effect pass: samples `input`, renders into `output`
    /// (whose texture uses `format`).
    pub fn render_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        desc: &EffectPassDesc,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        // A fresh uniform buffer per pass: queue-timeline writes into a
        // shared buffer would race passes recorded in the same submit.
        let params_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tpt-visual: effect params"),
                contents: bytemuck::bytes_of(&desc.params),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        // Curve LUT texture (256x1) when the pass carries one; otherwise
        // bind the neutral 1x1 LUT so the bind group stays complete.
        let curve_texture_view = desc.curve_lut.as_ref().map(|lut| {
            let data: Vec<u8> = lut.iter().flat_map(|e| e.iter().copied()).collect();
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tpt-visual: effect curve LUT"),
                size: wgpu::Extent3d {
                    width: 256,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                texture.as_image_copy(),
                &data,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(256 * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: 256,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            texture.create_view(&wgpu::TextureViewDescriptor::default())
        });
        let curve_view_ref = curve_texture_view.as_ref().unwrap_or(&self.neutral_lut);

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: effect bind group"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(curve_view_ref),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.curve_sampler),
                },
            ],
        });

        let pipeline = self.pipeline_for(desc.shader_source, format);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: effect pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
