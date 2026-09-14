//! OpenColorIO config compatibility (future work).
//!
//! The goal is to ingest OCIO v2 configs and expose their color space
//! transforms through [`crate::ColorPipeline`]. This module currently
//! provides only introspection helpers; the transform compiler lands with
//! the ACES/OCIO integration milestone (see DESIGN.md, roadmap).

use serde::{Deserialize, Serialize};

/// A minimal description of a color space entry parsed from an OCIO config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcioColorSpace {
    /// The space name as declared in the config.
    pub name: String,
    /// The family/grouping (e.g. "ACES", "Display").
    pub family: String,
}

/// Metadata about OCIO compatibility support in this version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcioSupport {
    /// Whether config parsing is implemented (currently `false`).
    pub config_parsing: bool,
    /// Whether `ColorPipeline` can compile OCIO file transforms (currently
    /// `false`).
    pub file_transforms: bool,
}

/// Returns the current OCIO support level.
#[must_use]
pub const fn ocio_support() -> OcioSupport {
    OcioSupport {
        config_parsing: false,
        file_transforms: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_unsupported() {
        let support = ocio_support();
        assert!(!support.config_parsing);
        assert!(!support.file_transforms);
    }
}
