//! WGSL shader management: embedded sources with compile caching.

use std::collections::HashMap;

/// Registry of the compositor's embedded WGSL shaders, compiled once per
/// device and cached.
pub struct ShaderRegistry {
    modules: HashMap<&'static str, wgpu::ShaderModule>,
}

/// All embedded shader sources, keyed by short name.
pub const SOURCES: &[(&str, &str)] = &[
    ("yuv_to_rgb", include_str!("../../shaders/yuv_to_rgb.wgsl")),
    ("transform", include_str!("../../shaders/transform.wgsl")),
    ("blend", include_str!("../../shaders/blend.wgsl")),
    ("mask", include_str!("../../shaders/mask.wgsl")),
    ("transition", include_str!("../../shaders/transition.wgsl")),
    ("blit", include_str!("../../shaders/blit.wgsl")),
];

impl ShaderRegistry {
    /// Creates the registry (empty until a module is requested).
    #[must_use]
    pub fn new() -> Self {
        ShaderRegistry {
            modules: HashMap::new(),
        }
    }

    /// Fetches (compiling on first use) the named shader module.
    pub fn module(&mut self, device: &wgpu::Device, name: &str) -> &wgpu::ShaderModule {
        if !self.modules.contains_key(name) {
            let source = SOURCES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| *s)
                .unwrap_or_else(|| panic!("unknown compositor shader: {name}"));
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            self.modules.insert(name_of(name), module);
        }
        &self.modules[name]
    }
}

fn name_of(name: &str) -> &'static str {
    SOURCES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(n, _)| *n)
        .expect("checked above")
}

impl Default for ShaderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
