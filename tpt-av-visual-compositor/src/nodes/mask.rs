//! Mask node: alpha masking (luma matte) and chroma keying.

use crate::gpu::device::Result;
use crate::gpu::pipeline::NodeParams;
use crate::node::{Affine2x3, CompositorNode, NodeFrame, NodeId};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Chroma key parameters for [`MaskNode`] (mirrors the shader contract).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromaKeyParams {
    /// Key color in 0..1 RGB.
    pub key_color: [f32; 3],
    /// Fully-keyed chroma distance.
    pub tolerance: f32,
    /// Feather width above the tolerance.
    pub softness: f32,
    /// Spill suppression (0..1).
    pub spill: f32,
}

/// Masks its input (slot 0).
///
/// - **Luma matte** (default): slot 1 is a matte image whose luminance
///   scales the input's alpha.
/// - **Chroma key**: no matte input; the input is keyed against
///   [`ChromaKeyParams`] with spill suppression (the Phase 4 keying
///   integration).
pub enum MaskNode {
    /// Luma matte against a mask input.
    Luma {
        /// The matte's target-UV → mask-UV affine.
        matrix: Affine2x3,
        input: Option<Arc<wgpu::TextureView>>,
        mask: Option<Arc<wgpu::TextureView>>,
    },
    /// Green/blue screen keying.
    Chroma {
        /// Key parameters.
        key: ChromaKeyParams,
        input: Option<Arc<wgpu::TextureView>>,
    },
}

impl MaskNode {
    /// A luma matte with an identity mask mapping.
    pub fn luma() -> Self {
        MaskNode::Luma {
            matrix: Affine2x3::IDENTITY,
            input: None,
            mask: None,
        }
    }

    /// A chroma keying mask.
    pub fn chroma(key: ChromaKeyParams) -> Self {
        MaskNode::Chroma {
            key,
            input: None,
        }
    }
}

impl CompositorNode for MaskNode {
    fn render(
        &mut self,
        ctx: &mut NodeFrame,
        output: &wgpu::TextureView,
        base: &NodeParams,
        _frame: u64,
    ) -> Result<()> {
        let (input, mask_view, mode, key_p1, spill, matrix) = match self {
            MaskNode::Luma { matrix, input, mask } => {
                let Some((i, m)) = input.clone().zip(mask.clone()) else {
                    return Err(crate::gpu::device::CompositorError::InvalidOperation(
                        "luma mask node requires input and matte".into(),
                    ));
                };
                (i, Some(m), 0_u32, [0.0; 4], 0.0_f32, *matrix)
            }
            MaskNode::Chroma { key, input } => {
                let Some(i) = input.clone() else {
                    return Err(crate::gpu::device::CompositorError::InvalidOperation(
                        "chroma mask node requires input".into(),
                    ));
                };
                (
                    i,
                    None,
                    1_u32,
                    [key.key_color[0], key.key_color[1], key.key_color[2], key.tolerance],
                    key.spill,
                    Affine2x3::IDENTITY,
                )
            }
        };

        let pipeline = ctx.pipelines.get(ctx.device, ctx.shaders, "mask", ctx.target_format)?;
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor::default());
        let mut params = base.clone();
        params.mode = mode;
        params.p0 = [spill, 0.0, 0.0, 0.0];
        params.p1 = key_p1;
        params.progress = match self {
            MaskNode::Chroma { key, .. } => key.softness,
            _ => 0.0,
        };
        params = params.with_affine2x3(&matrix);
        let uniform = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tpt-visual: mask params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_layout = pipeline.get_bind_group_layout(0);
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&input),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ];
        // Chroma mode leaves binding 3 unbound — legal because the shader
        // only reads it in luma mode... WGSL requires all declared bindings
        // be bound, so bind the input as a stand-in.
        let stand_in = input.clone();
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(
                mask_view.as_ref().unwrap_or(&stand_in),
            ),
        });
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tpt-visual: mask bind group"),
            layout: &bind_layout,
            entries: &entries,
        });
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tpt-visual: mask pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }

    fn inputs(&self) -> Vec<NodeId> {
        Vec::new()
    }

    fn set_input(&mut self, slot: usize, view: Arc<wgpu::TextureView>) {
        match self {
            MaskNode::Luma { input, mask, .. } => match slot {
                0 => *input = Some(view),
                1 => *mask = Some(view),
                _ => {}
            },
            MaskNode::Chroma { input, .. } => {
                if slot == 0 {
                    *input = Some(view);
                }
            }
        }
    }
}
