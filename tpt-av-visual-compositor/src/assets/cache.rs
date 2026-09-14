//! `VideoAssetCache`: per-asset decoded frame buffer, GPU texture map, and
//! background prefetching.

use crate::assets::decoder::FrameDecoder;
use crate::assets::proxies::{generate_proxy, ProxyConfig};
use crate::gpu::device::Result;
use crate::gpu::texture::GpuTexture;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tpt_av_visual_timeline::VideoAsset;
use tpt_av_visual_utils::VideoFrame;

/// Per-asset frame cache with background prefetching.
///
/// - `get_frame` is the render-thread entry point: cached GPU textures are
///   returned immediately; otherwise the frame is decoded (slow path) and
///   uploaded.
/// - `prefetch` spawns a background decoder thread for a frame range; the
///   render thread drains its results without blocking.
pub struct VideoAssetCache {
    asset: VideoAsset,
    decoder: Arc<Mutex<Box<dyn FrameDecoder>>>,
    cpu_frames: HashMap<u64, Arc<VideoFrame>>,
    gpu_textures: HashMap<u64, GpuTexture>,
    /// LRU order of GPU-resident frames (front = oldest).
    gpu_order: VecDeque<u64>,
    gpu_limit: usize,
    prefetch: Option<PrefetchHandle>,
    proxy: Option<ProxyConfig>,
    proxy_frames: HashMap<u64, Arc<VideoFrame>>,
}

struct PrefetchHandle {
    stop: Arc<AtomicBool>,
    ready: Arc<Mutex<HashMap<u64, Arc<VideoFrame>>>>,
    thread: Option<JoinHandle<()>>,
}

impl VideoAssetCache {
    /// Creates a cache for `asset` decoded by `decoder`.
    pub fn new(asset: VideoAsset, decoder: Box<dyn FrameDecoder>) -> Self {
        VideoAssetCache {
            asset,
            decoder: Arc::new(Mutex::new(decoder)),
            cpu_frames: HashMap::new(),
            gpu_textures: HashMap::new(),
            gpu_order: VecDeque::new(),
            gpu_limit: 24,
            prefetch: None,
            proxy: None,
            proxy_frames: HashMap::new(),
        }
    }

    /// The cached asset metadata.
    #[must_use]
    pub fn asset(&self) -> &VideoAsset {
        &self.asset
    }

    /// Number of CPU-cached frames (diagnostics).
    #[must_use]
    pub fn cpu_frame_count(&self) -> usize {
        self.cpu_frames.len()
    }

    /// Number of GPU-resident frames (diagnostics).
    #[must_use]
    pub fn gpu_frame_count(&self) -> usize {
        self.gpu_textures.len()
    }

    /// Enables proxy decoding at the given config.
    pub fn set_proxy(&mut self, proxy: Option<ProxyConfig>) {
        self.proxy = proxy;
        self.proxy_frames.clear();
    }

    /// The active proxy config, if any.
    #[must_use]
    pub const fn proxy(&self) -> Option<ProxyConfig> {
        self.proxy
    }

    /// Spawns a background thread decoding `[start, end)`.
    ///
    /// Any previous prefetch is stopped first. The thread stops on drop or
    /// [`VideoAssetCache::stop_prefetch`].
    pub fn prefetch(&mut self, start: u64, end: u64) {
        self.stop_prefetch();
        let stop = Arc::new(AtomicBool::new(false));
        let ready: Arc<Mutex<HashMap<u64, Arc<VideoFrame>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let decoder = self.decoder.clone();
        let proxy = self.proxy;
        let thread_stop = stop.clone();
        let thread_ready = ready.clone();
        let handle = std::thread::Builder::new()
            .name(format!("tpt-visual prefetch {}", self.asset.id))
            .spawn(move || {
                for index in start..end {
                    if thread_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    // Take the decoder lock only for the decode call so the
                    // render thread's slow path can interleave.
                    let decoded = {
                        let mut guard = decoder.lock().expect("prefetch decoder poisoned");
                        guard.decode_frame(index)
                    };
                    match decoded {
                        Ok(mut frame) => {
                            if let Some(config) = proxy {
                                let proxied = generate_proxy(&frame, config.target_height);
                                frame = proxied;
                            }
                            thread_ready
                                .lock()
                                .expect("prefetch ready poisoned")
                                .insert(index, Arc::new(frame));
                        }
                        Err(_) => return, // end of stream / decode failure
                    }
                }
            })
            .ok();
        self.prefetch = Some(PrefetchHandle {
            stop,
            ready,
            thread: handle,
        });
    }

    /// Stops any running prefetch thread, blocking until it exits.
    pub fn stop_prefetch(&mut self) {
        if let Some(mut handle) = self.prefetch.take() {
            handle.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = handle.thread.take() {
                let _ = thread.join();
            }
            // Keep whatever frames finished.
            if let Ok(frames) = handle.ready.lock() {
                for (index, frame) in frames.iter() {
                    self.cpu_frames.entry(*index).or_insert_with(|| frame.clone());
                }
            }
        }
    }

    fn drain_prefetched(&mut self) {
        if let Some(handle) = &self.prefetch {
            if let Ok(mut frames) = handle.ready.lock() {
                for (index, frame) in frames.drain() {
                    self.cpu_frames.insert(index, frame);
                }
            }
        }
    }

    /// Returns the GPU texture for `frame`, decoding + uploading on miss
    /// (slow path).
    pub fn get_frame(
        &mut self,
        frame: u64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        yuv_pipeline: &wgpu::RenderPipeline,
    ) -> Result<&GpuTexture> {
        self.drain_prefetched();

        // Fast path: GPU-resident.
        if !self.gpu_textures.contains_key(&frame) {
            self.decode_and_upload(frame, device, queue, encoder, yuv_pipeline)?;
        }
        // LRU touch.
        self.gpu_order.retain(|&f| f != frame);
        self.gpu_order.push_back(frame);
        self.evict_gpu();
        Ok(&self.gpu_textures[&frame])
    }

    fn decode_and_upload(
        &mut self,
        frame: u64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        yuv_pipeline: &wgpu::RenderPipeline,
    ) -> Result<()> {
        // 1. Proxy path.
        if self.proxy.is_some() {
            if !self.proxy_frames.contains_key(&frame) {
                let decoded = self.decode_sync(frame, false)?;
                self.proxy_frames.insert(
                    frame,
                    Arc::new(generate_proxy(
                        &decoded,
                        self.proxy.map_or(1080, |c| c.target_height),
                    )),
                );
            }
            let proxied = self.proxy_frames[&frame].clone();
            let texture = GpuTexture::upload(device, queue, encoder, &proxied, yuv_pipeline)?;
            self.gpu_textures.insert(frame, texture);
            return Ok(());
        }

        // 2. CPU-cached frame.
        if let Some(cached) = self.cpu_frames.get(&frame).cloned() {
            let texture = GpuTexture::upload(device, queue, encoder, &cached, yuv_pipeline)?;
            self.gpu_textures.insert(frame, texture);
            return Ok(());
        }

        // 3. Slow path: synchronous decode.
        let decoded = self.decode_sync(frame, true)?;
        let texture = GpuTexture::upload(device, queue, encoder, &decoded, yuv_pipeline)?;
        self.cpu_frames.insert(frame, Arc::new(decoded));
        self.gpu_textures.insert(frame, texture);
        Ok(())
    }

    fn decode_sync(
        &mut self,
        frame: u64,
        cache: bool,
    ) -> Result<VideoFrame> {
        let decoded = {
            let mut decoder = self
                .decoder
                .lock()
                .map_err(|_| crate::gpu::device::CompositorError::Decode(
                    "decoder lock poisoned".into(),
                ))?;
            decoder.decode_frame(frame)?
        };
        if cache {
            self.cpu_frames.insert(frame, Arc::new(decoded.clone()));
        }
        Ok(decoded)
    }

    fn evict_gpu(&mut self) {
        while self.gpu_textures.len() > self.gpu_limit {
            let Some(oldest) = self.gpu_order.pop_front() else {
                break;
            };
            self.gpu_textures.remove(&oldest);
        }
    }
}

impl Drop for VideoAssetCache {
    fn drop(&mut self) {
        self.stop_prefetch();
    }
}
