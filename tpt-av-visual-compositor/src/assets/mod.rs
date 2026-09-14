//! Asset management: decoders, frame caching, prefetching, and proxies.

pub mod cache;
pub mod decoder;
pub mod proxies;

pub use cache::VideoAssetCache;
pub use decoder::{FrameDecoder, ImageSequenceDecoder, ProceduralDecoder};
pub use proxies::{generate_proxy, ProxyConfig};

#[cfg(feature = "kinetix")]
pub use decoder::KinetixDecoder;
