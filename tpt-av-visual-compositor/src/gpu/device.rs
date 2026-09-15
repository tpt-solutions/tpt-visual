//! wgpu instance/adapter/device/queue initialization.

use std::sync::Arc;

/// Errors surfaced by the compositor.
#[derive(Debug, thiserror::Error)]
pub enum CompositorError {
    /// A GPU operation failed.
    #[error("GPU error: {0}")]
    Gpu(String),
    /// No GPU adapter is available.
    #[error("no compatible GPU device is available")]
    NoDevice,
    /// A rendering input or stage was misconfigured.
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
    /// Frame/asset data was not usable.
    #[error("decode error: {0}")]
    Decode(String),

    /// An I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, CompositorError>;

/// Human-readable GPU adapter description, for environment probes and
/// diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    /// Adapter name (e.g. "NVIDIA GeForce RTX 4070").
    pub adapter: String,
    /// Backend actually in use (Vulkan, Metal, D3D12, GL, ...).
    pub backend: String,
    /// Device type (DiscreteGpu, IntegratedGpu, Cpu, ...).
    pub device_type: String,
    /// Driver version string, when reported.
    pub driver: String,
}

/// Shared GPU context: instance, adapter, device, and queue behind `Arc`s so
/// the render thread, asset caches, and the effect renderer can share one
/// logical device.
#[derive(Clone)]
pub struct GpuContext {
    instance: Arc<wgpu::Instance>,
    adapter: Arc<wgpu::Adapter>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl GpuContext {
    /// Probes every backend for a headless device (tests, CLI renderers).
    /// Returns `None` when no adapter exists (e.g. GPU-less CI runners).
    #[must_use]
    pub fn headless() -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tpt-visual compositor device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .ok()?;
        device.on_uncaptured_error(Box::new(|err| {
            panic!("tpt-visual wgpu error: {err}");
        }));
        Some(Self {
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }

    /// Describes the active adapter (for `probe_gpu`-style diagnostics).
    #[must_use]
    pub fn info(&self) -> GpuInfo {
        let info = self.adapter.get_info();
        GpuInfo {
            adapter: info.name,
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: info.driver,
        }
    }

    /// The logical device.
    #[must_use]
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// The command queue.
    #[must_use]
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// The adapter (for capability reporting).
    #[must_use]
    pub fn adapter(&self) -> &Arc<wgpu::Adapter> {
        &self.adapter
    }

    /// The instance (for surface creation by players).
    #[must_use]
    pub fn instance(&self) -> &Arc<wgpu::Instance> {
        &self.instance
    }
}
