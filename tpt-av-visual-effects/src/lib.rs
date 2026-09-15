//! `tpt-av-visual-effects` — GPU-accelerated video effects for the TPT AV
//! visual stack.
//!
//! Every effect ships a CPU reference implementation (used for validation
//! and software fallbacks) and one or more WGSL GPU passes. All shaders live
//! in `src/gpu_shaders/` and share the [`effect::EffectParams`] uniform
//! layout, so a single [`effect::EffectRenderer`] can execute any effect
//! chain.
//!
//! # Registry
//!
//! The timeline stores effects as name + numeric parameter bag
//! (`EffectInstance`); [`build_effect`] resolves those to concrete
//! implementations. The registered names:
//!
//! `gaussian_blur`, `box_blur`, `motion_blur`, `sharpen`, `color_correct`,
//! `levels`, `curves`, `chroma_key`, `noise`, `noise_reduction`, `vignette`.

pub mod effect;
pub mod effects;

pub use effect::{Effect, EffectError, EffectParams, EffectPassDesc, EffectRenderer, Result};
pub use effects::{
    BoxBlur, ChromaKey, ColorCorrect, Curve, GaussianBlur, Levels, MotionBlur, Noise,
    NoiseReduction, Sharpen, ToneCurve, Vignette,
};

use std::collections::BTreeMap;
use std::sync::Arc;

/// The numeric parameter bag carried by timeline `EffectInstance`s.
pub type ParamBag = BTreeMap<String, f32>;

/// Builds a concrete effect from a registered name and parameter bag.
///
/// # Errors
/// Returns [`EffectError::UnknownEffect`] for unregistered names.
pub fn build_effect(name: &str, params: &ParamBag) -> Result<Box<dyn Effect>> {
    match name {
        "gaussian_blur" | "box_blur" | "motion_blur" => Ok(effects::blur::from_params(
            match name {
                "gaussian_blur" => "gaussian_blur",
                "box_blur" => "box_blur",
                _ => "motion_blur",
            },
            params,
        )),
        "sharpen" => Ok(Box::new(Sharpen::new(effects::param(
            params, "amount", 1.0,
        )))),
        "color_correct" => Ok(effects::color_correct::from_params(params)),
        "levels" => Ok(effects::levels::from_params(params)),
        "curves" => Ok(effects::curves::from_params(params)),
        "chroma_key" => Ok(effects::chroma_key::from_params(params)),
        "noise" | "noise_reduction" => Ok(effects::noise::from_params(
            match name {
                "noise" => "noise",
                _ => "noise_reduction",
            },
            params,
        )),
        "vignette" => Ok(effects::vignette::from_params(params)),
        other => Err(EffectError::UnknownEffect(other.into())),
    }
}

/// Every registered effect name.
#[must_use]
pub fn registered_effects() -> &'static [&'static str] {
    &[
        "gaussian_blur",
        "box_blur",
        "motion_blur",
        "sharpen",
        "color_correct",
        "levels",
        "curves",
        "chroma_key",
        "noise",
        "noise_reduction",
        "vignette",
    ]
}

/// Initializes a headless GPU device for tests and examples.
///
/// Returns `None` when no compatible adapter exists (e.g. GPU-less CI);
/// GPU-dependent tests skip themselves in that case.
#[must_use]
pub fn headless_gpu() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("tpt-visual effects headless device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        },
        None,
    ))
    .ok()?;
    Some((Arc::new(device), Arc::new(queue)))
}
