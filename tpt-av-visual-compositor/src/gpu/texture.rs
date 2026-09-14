//! GPU texture management: [`GpuTexture`] upload, frame conversion
//! (YUV → RGBA on the GPU), and the [`TexturePool`] for reuse across frames.

use crate::gpu::device::{CompositorError, Result};
use std::collections::HashMap;
use tpt_av_visual_utils::{PixelFormat, Resolution, VideoFrame};

const YUV_WGSL: &str = include_str!("../../shaders/yuv_to_rgb.wgsl");

/// A GPU texture with its view and sampler, holding one video frame.
pub struct GpuTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    resolution: Resolution,
    format: wgpu::TextureFormat,
}

impl GpuTexture {
    /// Uploads a CPU-side frame to the GPU, converting planar YUV to packed
    /// RGBA with a GPU pass (BT.709 limited range, the broadcast default).
    /// RGBA frames are uploaded directly.
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &VideoFrame,
        yuv_pipeline: &wgpu::RenderPipeline,
    ) -> Result<Self> {
        match frame.pixel_format {
            PixelFormat::Rgba8 => Ok(Self::upload_rgba_raw(
                device,
                queue,
                frame.width,
                frame.height,
                &frame.data,
            )),
            PixelFormat::Yuv420p | PixelFormat::Yuv422p | PixelFormat::Yuv444p => {
                let planes = upload_planes(device, queue, frame)?;
                let target = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("tpt-visual: converted frame"),
                    size: wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
                run_yuv_conversion(device, encoder, yuv_pipeline, &planes, &target_view)?;
                Ok(Self::from_texture(device, target))
            }
            other => Err(CompositorError::Decode(format!(
                "unsupported source pixel format {other}"
            ))),
        }
    }

    /// Uploads packed RGBA data without conversion.
    #[must_use]
    pub fn upload_rgba_raw(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tpt-visual: frame texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            rgba,
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
        Self::from_texture(device, texture)
    }

    /// Wraps an existing texture (e.g. from the pool) with view + sampler.
    #[must_use]
    pub fn from_texture(device: &wgpu::Device, texture: wgpu::Texture) -> Self {
        let resolution = Resolution::new(texture.width(), texture.height())
            .unwrap_or(Resolution::new(1, 1).expect("1x1 always valid"));
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tpt-visual: frame sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        GpuTexture {
            texture,
            view,
            sampler,
            resolution,
            format: wgpu::TextureFormat::Rgba8Unorm,
        }
    }

    /// The texture view (for sampling as a node input).
    #[must_use]
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The sampler.
    #[must_use]
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// The underlying texture.
    #[must_use]
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Resolution of the frame.
    #[must_use]
    pub const fn resolution(&self) -> Resolution {
        self.resolution
    }

    /// Pixel format of the GPU texture.
    #[must_use]
    pub const fn format(&self) -> wgpu::TextureFormat {
        self.format
    }
}

/// Per-plane textures for planar YUV frames.
pub struct YuvPlanes {
    /// Y (luma) plane view.
    pub y: wgpu::TextureView,
    /// U (blue chroma) plane view.
    pub u: wgpu::TextureView,
    /// V (red chroma) plane view.
    pub v: wgpu::TextureView,
    /// Chroma subsampling: 0 = 4:4:4, 1 = 4:2:2, (1,1) = 4:2:0.
    pub chroma_shift: (u32, u32),
    _textures: [wgpu::Texture; 3],
}

fn upload_planes(device: &wgpu::Device, queue: &wgpu::Queue, frame: &VideoFrame) -> Result<YuvPlanes> {
    let make_plane = |w: u32, h: u32, data: &[u8], label: &str| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        texture
    };

    let y_plane = frame.plane(0).map_err(|e| CompositorError::Decode(e.to_string()))?;
    let u_plane = frame.plane(1).map_err(|e| CompositorError::Decode(e.to_string()))?;
    let v_plane = frame.plane(2).map_err(|e| CompositorError::Decode(e.to_string()))?;

    let chroma_shift = match frame.pixel_format {
        PixelFormat::Yuv420p => (1, 1),
        PixelFormat::Yuv422p => (1, 0),
        _ => (0, 0),
    };
    let cw = (frame.width + chroma_shift.0) >> chroma_shift.0;
    let ch = (frame.height + chroma_shift.1) >> chroma_shift.1;

    let y_tex = make_plane(frame.width, frame.height, y_plane, "tpt-visual: Y plane");
    let u_tex = make_plane(cw, ch, u_plane, "tpt-visual: U plane");
    let v_tex = make_plane(cw, ch, v_plane, "tpt-visual: V plane");

    let view_desc = wgpu::TextureViewDescriptor::default();
    Ok(YuvPlanes {
        y: y_tex.create_view(&view_desc),
        u: u_tex.create_view(&view_desc),
        v: v_tex.create_view(&view_desc),
        chroma_shift,
        _textures: [y_tex, u_tex, v_tex],
    })
}

/// Runs the YUV → RGBA conversion pass into `target`.
///
/// # Errors
/// Returns [`CompositorError::Gpu`] if pipeline resources cannot be built.
pub fn run_yuv_conversion(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    planes: &YuvPlanes,
    target: &wgpu::TextureView,
) -> Result<()> {
    let bind_layout = pipeline.get_bind_group_layout(0);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tpt-visual: yuv bind group"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&planes.y),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&planes.u),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&planes.v),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("tpt-visual: yuv conversion"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
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
    Ok(())
}

/// The WGSL source for the YUV conversion pass (exposed for pipeline
/// construction).
#[must_use]
pub const fn yuv_shader_source() -> &'static str {
    YUV_WGSL
}

/// A GPU texture pool: recycles render-target textures of equal size/format
/// across frames instead of reallocating.
///
/// Textures are handed out as [`wgpu::Texture`] handles; the pool keeps the
/// device alive. `release` returns a texture to the pool for reuse.
#[derive(Default)]
pub struct TexturePool {
    available: HashMap<PoolKey, Vec<wgpu::Texture>>,
    live_count: usize,
    reuse_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PoolKey {
    width: u32,
    height: u32,
    usage: wgpu::TextureUsages,
}

impl TexturePool {
    /// An empty pool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquires a texture of the given size/usage — reused when possible.
    #[must_use]
    pub fn acquire(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        let key = PoolKey {
            width,
            height,
            usage,
        };
        if let Some(texture) = self.available.get_mut(&key).and_then(Vec::pop) {
            self.reuse_count += 1;
            return texture;
        }
        self.live_count += 1;
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tpt-visual: pooled texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: usage
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    /// Returns a texture to the pool.
    pub fn release(&mut self, texture: wgpu::Texture) {
        let key = PoolKey {
            width: texture.width(),
            height: texture.height(),
            usage: texture.usage(),
        };
        let bucket = self.available.entry(key).or_default();
        // Cap the cache so pathological sizes cannot accumulate.
        if bucket.len() < 8 {
            bucket.push(texture);
        }
    }

    /// Number of textures created (reuse counter for diagnostics).
    #[must_use]
    pub const fn stats(&self) -> (usize, u64) {
        (self.live_count, self.reuse_count)
    }
}
