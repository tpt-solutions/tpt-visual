//! GPU plumbing for [`ColorPipeline`]: a cached render pipeline running the
//! fused `color_convert.wgsl` pass.

use crate::luts::Lut3D;
use crate::ColorPipeline;
use bytemuck::Pod;
use bytemuck::Zeroable;
use wgpu::util::DeviceExt;

/// Uniform block mirroring `ColorParams` in `color_convert.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GpuParams {
    /// `src_to_dst` matrix columns (each padded to vec4 for WGSL layout).
    src_to_dst_cols: [[f32; 4]; 3],
    in_gamma: f32,
    out_gamma: f32,
    in_scale: f32,
    in_transfer: u32,
    out_transfer: u32,
    tonemap_mode: u32,
    use_lut: u32,
    lut_size: f32,
    // WGSL rounds the struct to 16-byte alignment for the trailing vec3;
    // pad to the same 96-byte stride on the Rust side.
    pad: [f32; 4],
}

/// WGSL mat3x3 columns from a row-major 3x3.
fn matrix_columns(m: &[[f32; 3]; 3]) -> [[f32; 4]; 3] {
    [
        [m[0][0], m[1][0], m[2][0], 0.0],
        [m[0][1], m[1][1], m[2][1], 0.0],
        [m[0][2], m[1][2], m[2][2], 0.0],
    ]
}

const SHADER: &str = include_str!("../shaders/color_convert.wgsl");

/// A device-resident [`ColorPipeline`]. Create once per (pipeline, format)
/// and reuse across frames; [`GpuColorPipeline::apply`] then only records
/// commands.
pub struct GpuColorPipeline {
    pipeline: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    lut_texture: wgpu::Texture,
    lut_view: wgpu::TextureView,
    bind_layout: wgpu::BindGroupLayout,
    input_sampler: wgpu::Sampler,
    lut_sampler: wgpu::Sampler,
}

impl GpuColorPipeline {
    /// Compiles the pipeline and uploads uniform/LUT data.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &ColorPipeline,
        target_format: wgpu::TextureFormat,
    ) -> crate::Result<Self> {
        let converter = pipeline.gamut_converter()?;
        let (tonemap_mode, use_lut) = (
            pipeline
                .tone_mapper
                .as_ref()
                .map_or(0_u32, crate::ToneMapper::as_u32),
            u32::from(pipeline.lut.is_some()),
        );
        let params = GpuParams {
            src_to_dst_cols: matrix_columns(&converter.matrix()),
            in_gamma: pipeline.input_transfer.gamma().unwrap_or(1.0),
            out_gamma: pipeline.output_transfer.gamma().unwrap_or(1.0),
            in_scale: pipeline.input_linear_scale,
            in_transfer: pipeline.input_transfer.as_u32(),
            out_transfer: pipeline.output_transfer.as_u32(),
            tonemap_mode,
            use_lut,
            lut_size: pipeline.lut.as_ref().map_or(2.0, |l| l.size as f32),
            pad: [0.0; 4],
        };
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tpt-visual: color params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // LUT texture (or a neutral 1x1x1 dummy when unused).
        let lut_data: Vec<[f32; 3]> = match &pipeline.lut {
            Some(lut) => lut.data.clone(),
            None => vec![[0.0; 3]; 1],
        };
        let lut_size = match &pipeline.lut {
            Some(lut) => lut.size as u32,
            None => 1,
        };
        let lut_extent = wgpu::Extent3d {
            width: lut_size,
            height: lut_size,
            depth_or_array_layers: lut_size,
        };
        let lut_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tpt-visual: color 3D LUT"),
            size: lut_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            // rgba16float is the filterable float format (rgba32float is not
            // filterable in WebGPU); half precision is plenty for LUTs.
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Interleave to RGBA16F for upload.
        let mut rgba: Vec<u16> = Vec::with_capacity(lut_data.len() * 4);
        for entry in &lut_data {
            rgba.extend_from_slice(&[
                f32_to_f16(entry[0]),
                f32_to_f16(entry[1]),
                f32_to_f16(entry[2]),
                f32_to_f16(1.0),
            ]);
        }
        queue.write_texture(
            lut_texture.as_image_copy(),
            bytemuck::cast_slice(&rgba),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(lut_size * 8),
                rows_per_image: Some(lut_size),
            },
            lut_extent,
        );
        let lut_view = lut_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tpt-visual: color_convert.wgsl"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tpt-visual: color bind layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
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

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tpt-visual: color pipeline layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tpt-visual: color pipeline"),
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
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let input_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tpt-visual: color input sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..wgpu::SamplerDescriptor::default()
        });
        let lut_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tpt-visual: color LUT sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..wgpu::SamplerDescriptor::default()
        });

        Ok(GpuColorPipeline {
            pipeline,
            params: params_buf,
            lut_texture,
            lut_view,
            bind_layout,
            input_sampler,
            lut_sampler,
        })
    }

    /// Replaces the 3D LUT contents without rebuilding the pipeline.
    pub fn update_lut(&self, queue: &wgpu::Queue, lut: &Lut3D) {
        let size = lut.size as u32;
        let mut rgba: Vec<u16> = Vec::with_capacity(lut.data.len() * 4);
        for entry in &lut.data {
            rgba.extend_from_slice(&[
                f32_to_f16(entry[0]),
                f32_to_f16(entry[1]),
                f32_to_f16(entry[2]),
                f32_to_f16(1.0),
            ]);
        }
        queue.write_texture(
            self.lut_texture.as_image_copy(),
            bytemuck::cast_slice(&rgba),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(size * 8),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
        );
    }

    /// Records the color pass: reads `input`, writes `output`.
    pub fn apply(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: color bind group"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.input_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.lut_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.lut_sampler),
                },
            ],
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: color pass"),
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Converts an f32 to IEEE 754 binary16 bits (round-to-nearest-even).
fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;

    if exp >= 0x1f {
        // Inf/NaN (clamped exponent) or overflow → saturate to inf.
        return sign | 0x7c00;
    }
    if exp >= 0x1e {
        // Overflow → infinity.
        return sign | 0x7c00;
    }
    if exp <= 0 {
        if exp < -10 {
            // Underflow to zero.
            return sign;
        }
        // Subnormal half.
        let mantissa = mantissa | 0x0080_0000;
        let shift = (14 - exp) as u32;
        let half_m = mantissa >> shift;
        // Round to nearest even.
        let round_bit = 1 << (shift - 1);
        if (mantissa & round_bit) != 0 && (mantissa & (round_bit - 1) | half_m) != 0 {
            return sign | ((half_m + 1) as u16);
        }
        return sign | (half_m as u16);
    }
    let mut half_exp = (exp as u32) << 10;
    let mut half_m = mantissa >> 13;
    // Round to nearest even.
    if (mantissa & 0x1000) != 0 && (mantissa & 0x0fff != 0 || half_m & 1 != 0) {
        half_m += 1;
        if half_m == 0x0400 {
            half_m = 0;
            half_exp += 1 << 10;
        }
    }
    sign | half_exp as u16 | half_m as u16
}

/// Creates a headless device+queue, probing every backend.
#[must_use]
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    let (device, queue) = pollster::block_on(
        adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tpt-visual headless device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ),
    )
    .ok()?;
    Some((device, queue))
}
