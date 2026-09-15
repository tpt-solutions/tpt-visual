//! The crate-wide error type.

/// Errors produced anywhere in the TPT AV visual stack.
#[derive(Debug, thiserror::Error)]
pub enum VisualError {
    /// A GPU (wgpu) operation failed.
    #[error("GPU error: {0}")]
    Gpu(String),

    /// No suitable GPU adapter / device could be initialized.
    #[error("no compatible GPU device is available")]
    NoDevice,

    /// A decoder or demuxer failed to produce usable data.
    #[error("decode error: {0}")]
    Decode(String),

    /// Frame data does not match its declared format or dimensions.
    #[error("invalid frame: {0}")]
    InvalidFrame(String),

    /// The requested pixel format is not supported for this operation.
    #[error("unsupported pixel format: {0}")]
    UnsupportedPixelFormat(String),

    /// A timeline/asset reference could not be resolved.
    #[error("not found: {0}")]
    NotFound(String),

    /// A requested operation is not valid for the current state.
    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    /// A serialization/deserialization failure.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// An I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl VisualError {
    /// Convenience constructor for [`VisualError::Gpu`].
    pub fn gpu(msg: impl Into<String>) -> Self {
        VisualError::Gpu(msg.into())
    }

    /// Convenience constructor for [`VisualError::InvalidFrame`].
    pub fn invalid_frame(msg: impl Into<String>) -> Self {
        VisualError::InvalidFrame(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_readably() {
        assert_eq!(
            VisualError::NoDevice.to_string(),
            "no compatible GPU device is available"
        );
        assert_eq!(VisualError::gpu("boom").to_string(), "GPU error: boom");
    }

    #[test]
    fn io_error_converts() {
        let err: VisualError = std::io::Error::other("disk").into();
        assert!(err.to_string().contains("disk"));
    }
}
