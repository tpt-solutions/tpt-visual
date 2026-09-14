//! Render pipeline creation and caching for compositing node passes.
//!
//! Every node pass shares one bind-group layout:
//!
//! - binding 0: input texture A (float, filterable)
//! - binding 1: sampler
//! - binding 2: uniform `NodeParams`
//! - binding 3: input texture B (second input for blend/mask/transition;
//!   unused entries are legal for single-input shaders)
//!
//! The uniform block is [`NodeParams`], a 96-byte WGSL-aligned struct whose
//! fields each shader interprets for its own purpose.

use crate::gpu::device::Result;
use std::collections::HashMap;
use std::sync::Arc;

/// Uniform block shared by every compositing node shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NodeParams {
    /// Column-major 3x3 (padded) transform: source UV → target UV.
    pub matrix_cols: [[f32; 4]; 3],
    /// First parameter quad (shader-specific).
    pub p0: [f32; 4],
    /// Second parameter quad (shader-specific).
    pub p1: [f32; 4],
    /// `1/width`, `1/height` of the target.
    pub texel: [f32; 2],
    /// Shader mode selector (blend mode, transition kind, ...).
    pub mode: u32,
    /// Progress / extra scalar.
    pub progress: f32,
}

impl NodeParams {
    /// Defaults: identity matrix, zero params, mode 0.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        NodeParams {
            matrix_cols: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            p0: [0.0; 4],
            p1: [0.0; 4],
            texel: [1.0 / width as f32, 1.0 / height as f32],
            mode: 0,
            progress: 0.0,
        }
    }

    /// Packs a row-major `[[a b tx] [c d ty]]` affine matrix into columns.
    #[must_use]
    pub fn with_affine2x3(mut self, m: &crate::node::Affine2x3) -> Self {
        // WGSL mat3x3 columns: [a b 0], [c d 0], [tx ty 1] applied to
        // homogeneous uvw.
        self.matrix_cols = [
            [m.a, m.b, 0.0, 0.0],
            [m.c, m.d, 0.0, 0.0],
            [m.tx, m.ty, 1.0, 0.0],
        ];
        self
    }
}

/// Cache of `(shader name, target format)` → pipeline.
pub struct PipelineCache {
    pipelines: HashMap<(&'static str, wgpu::TextureFormat), Arc<wgpu::RenderPipeline>>,
}

impl PipelineCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        PipelineCache {
            pipelines: HashMap::new(),
        }
    }

    /// Fetches (building on first use) the fullscreen pipeline for `name`
    /// rendering into `format`.
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        shaders: &mut crate::gpu::shader::ShaderRegistry,
        name: &'static str,
        format: wgpu::TextureFormat,
    ) -> Result<Arc<wgpu::RenderPipeline>> {
        if !self.pipelines.contains_key(&(name, format)) {
            let shader = shaders.module(device, name);
            let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tpt-visual: node bind layout"),
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
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tpt-visual: node pipeline layout"),
                bind_group_layouts: &[&bind_layout],
                push_constant_ranges: &[],
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
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
            self.pipelines.insert((name, format), Arc::new(pipeline));
        }
        self.pipelines
            .get(&(name, format))
            .cloned()
            .ok_or_else(|| crate::gpu::device::CompositorError::Gpu(format!(
                "pipeline {name} missing after insert"
            )))
    }

    /// Builds the YUV→RGBA conversion pipeline (different bind layout).
    pub fn get_yuv(
        &mut self,
        device: &wgpu::Device,
        shaders: &mut crate::gpu::shader::ShaderRegistry,
    ) -> Result<Arc<wgpu::RenderPipeline>> {
        if let Some(p) = self.pipelines.get(&("yuv_to_rgb", wgpu::TextureFormat::Rgba8Unorm)) {
            return Ok(p.clone());
        }
        let shader = shaders.module(device, "yuv_to_rgb");
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tpt-visual: yuv bind layout"),
            entries: &[
                plane_entry(0),
                plane_entry(1),
                plane_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tpt-visual: yuv pipeline layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tpt-visual: yuv pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let pipeline = Arc::new(pipeline);
        self.pipelines
            .insert(("yuv_to_rgb", wgpu::TextureFormat::Rgba8Unorm), pipeline.clone());
        Ok(pipeline)
    }
}

fn plane_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

impl Default for PipelineCache {
    fn default() -> Self {
        Self::new()
    }
}
