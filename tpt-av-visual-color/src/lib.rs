//! `tpt-av-visual-color` — color science, HDR tone mapping, and LUT
//! processing for the TPT AV visual stack.
//!
//! The central type is [`ColorPipeline`]: a five-step conversion
//!
//! 1. **Linearize** the input transfer function (sRGB, PQ, HLG, gamma).
//! 2. **Gamut-convert** between color spaces (Rec.709 ↔ Rec.2020 ↔ P3 ↔
//!    ACES) through XYZ with Bradford white adaptation.
//! 3. **Tone map** HDR to SDR (Reinhard, ACES filmic, custom curves).
//! 4. **Apply** an optional 3D LUT.
//! 5. **Encode** to the output transfer function.
//!
//! Every step runs on the CPU (scalar reference implementation, used for
//! validation against reference values) and on the GPU via one fused WGSL
//! pass ([`ColorPipeline::apply`]).
//!
//! # Validation
//!
//! Transfer-function math is checked against reference anchors from SMPTE ST
//! 2084, ITU-R BT.2100, IEC 61966-2-1, and the ACES specifications; gamut
//! matrices are checked against published constants and the official ACES
//! AP1↔XYZ transforms. See the `tests/` directory.

// Spec constants (ACES/BT.2100/ST 2084 anchors) intentionally carry more
// digits than f32 can hold so the published values stay greppable.
#![allow(clippy::excessive_precision)]

pub mod aces;
pub mod color_space;
pub mod gamut;
pub mod hdr;
pub mod luts;
pub mod ocio;
pub mod transfer;

pub mod gpu;

pub use color_space::{apply3, ColorSpace, Primaries};
pub use gamut::{bradford, GamutConverter};
pub use gpu::GpuColorPipeline;
pub use hdr::ToneMapper;
pub use luts::{Cube, Lut1D, Lut3D};
pub use transfer::TransferFunction;

pub use tpt_av_visual_utils::VisualError;

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, VisualError>;

/// Initializes a headless GPU device for tests and examples.
///
/// Returns `None` when no compatible adapter is available (e.g. CI runners
/// without a GPU); GPU-dependent tests skip themselves in that case.
#[must_use]
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    gpu::headless_device()
}

/// The color processing pipeline: input/output spaces and transfers, an
/// optional tone mapper, and an optional 3D LUT.
#[derive(Debug, Clone)]
pub struct ColorPipeline {
    /// Input color space.
    pub input_space: ColorSpace,
    /// Input transfer function.
    pub input_transfer: TransferFunction,
    /// Output color space.
    pub output_space: ColorSpace,
    /// Output transfer function.
    pub output_transfer: TransferFunction,
    /// Tone mapper for HDR → SDR conversion (`None` = passthrough).
    pub tone_mapper: Option<ToneMapper>,
    /// 3D LUT applied between tone mapping and output encoding.
    pub lut: Option<Lut3D>,
    /// Multiplier applied to linearized input light. Use this to map the
    /// PQ/HLG nominal range onto the SDR working range — e.g. per ITU-R
    /// BT.2408, scale PQ input by `1 / 0.0203` so 203-nit reference white
    /// maps to 1.0 before tone mapping. Defaults to 1.0.
    pub input_linear_scale: f32,
}

impl ColorPipeline {
    /// Creates a pipeline description.
    #[must_use]
    pub const fn new(
        input_space: ColorSpace,
        input_transfer: TransferFunction,
        output_space: ColorSpace,
        output_transfer: TransferFunction,
    ) -> Self {
        ColorPipeline {
            input_space,
            input_transfer,
            output_space,
            output_transfer,
            tone_mapper: None,
            lut: None,
            input_linear_scale: 1.0,
        }
    }

    /// Builder-style linear input scale (see the field docs).
    #[must_use]
    pub fn with_input_linear_scale(mut self, scale: f32) -> Self {
        self.input_linear_scale = scale;
        self
    }

    /// Builder-style tone mapper setter.
    #[must_use]
    pub fn with_tone_mapper(mut self, tone_mapper: ToneMapper) -> Self {
        self.tone_mapper = Some(tone_mapper);
        self
    }

    /// Builder-style LUT setter.
    #[must_use]
    pub fn with_lut(mut self, lut: Lut3D) -> Self {
        self.lut = Some(lut);
        self
    }

    /// The composed linear-light gamut conversion (input space → output
    /// space).
    pub fn gamut_converter(&self) -> Result<GamutConverter> {
        GamutConverter::new(self.input_space, self.output_space)
    }

    /// Applies the full pipeline to one pixel of **code-domain** color
    /// (encoded values in, encoded values out). This is the CPU reference
    /// implementation of the five steps.
    #[must_use]
    pub fn apply_pixel(&self, code: [f32; 3]) -> [f32; 3] {
        // 1. Linearize (plus optional HDR range scale).
        let mut linear = self.input_transfer.decode3(code);
        if self.input_linear_scale != 1.0 {
            linear = linear.map(|v| v * self.input_linear_scale);
        }
        // 2. Gamut conversion (unclamped: out-of-gamut HDR is legal data).
        if let Ok(conv) = self.gamut_converter() {
            linear = conv.convert_unclamped(linear);
        }
        // 3. Tone map.
        if let Some(tm) = &self.tone_mapper {
            linear = [tm.map(linear[0]), tm.map(linear[1]), tm.map(linear[2])];
        }
        // 4. 3D LUT.
        if let Some(lut) = &self.lut {
            linear = lut.sample(linear);
        }
        // 5. Encode.
        self.output_transfer.encode3(linear)
    }

    /// Applies the pipeline to a packed RGBA8 buffer in place.
    pub fn apply_rgba8(&self, rgba: &mut [u8]) {
        for px in rgba.chunks_exact_mut(4) {
            let code = [
                f32::from(px[0]) / 255.0,
                f32::from(px[1]) / 255.0,
                f32::from(px[2]) / 255.0,
            ];
            let out = self.apply_pixel(code);
            for (slot, v) in px.iter_mut().take(3).zip(out) {
                *slot = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

    /// Applies the pipeline on the GPU, reading `input` and writing
    /// `output` (both RGBA8-unorm-compatible views) in one fused pass.
    ///
    /// Convenience wrapper that compiles the pipeline for this call; hosts
    /// rendering many frames should hold a
    /// [`GpuColorPipeline`] instead.
    pub fn apply(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
        target_format: wgpu::TextureFormat,
    ) -> Result<()> {
        let gpu_pipeline = gpu::GpuColorPipeline::new(device, queue, self, target_format)?;
        gpu_pipeline.apply(device, encoder, input, output);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn identity_pipeline_is_identity() {
        let p = ColorPipeline::new(
            ColorSpace::Srgb,
            TransferFunction::Srgb,
            ColorSpace::Srgb,
            TransferFunction::Srgb,
        );
        for code in [[0.0, 0.5, 1.0], [0.18, 0.73, 0.04]] {
            let out = p.apply_pixel(code);
            for (o, i) in out.iter().zip(code) {
                assert!(close(*o, i, 1e-5), "{code:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn grey_stays_grey_through_gamut_and_tonemap() {
        // Rec.709 → Rec.2020 with Reinhard tone mapping: neutrals must stay
        // neutral (all channels map identically).
        let p = ColorPipeline::new(
            ColorSpace::Linear,
            TransferFunction::Linear,
            ColorSpace::Rec2020,
            TransferFunction::Srgb,
        )
        .with_tone_mapper(ToneMapper::Reinhard);
        let out = p.apply_pixel([0.5, 0.5, 0.5]);
        assert!(
            close(out[0], out[1], 1e-5) && close(out[1], out[2], 1e-5),
            "{out:?}"
        );
    }

    #[test]
    fn pq_to_srgb_hdr_pipeline_anchors() {
        // 100-nit PQ grey → sRGB code. 100 nits is SDR reference white, so
        // after ACES filmic tone mapping the mid output should sit in a
        // filmic-but-bright range.
        let p = ColorPipeline::new(
            ColorSpace::Rec2020,
            TransferFunction::Pq,
            ColorSpace::Srgb,
            TransferFunction::Srgb,
        )
        .with_tone_mapper(ToneMapper::AcesFilmic);
        // BT.2408: 203 nits is the SDR reference white in HDR masters, so
        // scale PQ linear light by 1/0.0203 before tone mapping.
        let p = p.with_input_linear_scale(1.0 / 0.0203);
        let code = TransferFunction::Pq.encode(0.0203); // 203 nits
        let out = p.apply_pixel([code, code, code]);
        // Tone-mapped reference white: ACES filmic(1.0) ≈ 0.8038 linear,
        // then sRGB-encoded to ≈ 0.9084.
        assert!(close(out[0], 0.908_4, 5e-3), "{out:?}");
        assert!(close(out[0], out[2], 1e-5), "neutral stays neutral");
    }

    #[test]
    fn lut_is_applied_inside_the_pipeline() {
        let lut = Lut3D::identity(8).unwrap();
        let p = ColorPipeline::new(
            ColorSpace::Srgb,
            TransferFunction::Srgb,
            ColorSpace::Srgb,
            TransferFunction::Srgb,
        )
        .with_lut(lut);
        let code = [0.2, 0.6, 0.9];
        let out = p.apply_pixel(code);
        for (o, i) in out.iter().zip(code) {
            assert!(close(*o, i, 5e-2), "{code:?} -> {out:?}");
        }
    }

    #[test]
    fn rgba8_in_place() {
        let p = ColorPipeline::new(
            ColorSpace::Srgb,
            TransferFunction::Srgb,
            ColorSpace::Srgb,
            TransferFunction::Srgb,
        );
        let mut buf = vec![128_u8, 128, 128, 255, 0, 255, 10, 200];
        let original = buf.clone();
        p.apply_rgba8(&mut buf);
        // Identity pipeline keeps values (within 8-bit rounding).
        for (o, i) in buf.iter().zip(original) {
            assert!((i32::from(*o) - i32::from(i)).abs() <= 1, "{buf:?}");
        }
    }

    #[test]
    fn rec709_camera_to_srgb_display() {
        // Typical camera footage: Rec.709 primaries, 2.4 gamma encode →
        // sRGB display. Mid-grey camera code maps to near-mid sRGB code.
        let p = ColorPipeline::new(
            ColorSpace::Rec709,
            TransferFunction::Gamma(2.4),
            ColorSpace::Srgb,
            TransferFunction::Srgb,
        );
        let code = TransferFunction::Gamma(2.4).encode(0.18);
        let out = p.apply_pixel([code, code, code]);
        // Same primaries + same white: decoding the output recovers 18%
        // display-linear exactly (only the curve differs).
        let linear_out = TransferFunction::Srgb.decode(out[0]);
        assert!(close(out[0], out[2], 1e-5));
        assert!(close(linear_out, 0.18, 1e-3), "{out:?}");
    }
}
