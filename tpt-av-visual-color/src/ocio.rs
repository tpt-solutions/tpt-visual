//! OpenColorIO config compatibility.
//!
//! Full OCIO v2 transform compilation is future work; what exists today is a
//! **best-effort config scanner** that extracts the color spaces, displays,
//! and roles declared in a `.ocio` (XML) config so host applications can
//! enumerate a user's OCIO environment and map it onto
//! [`crate::ColorPipeline`] inputs. Attribute parsing is deliberately
//! minimal (key="value" scanning) — OCIO configs that rely on YAML syntax or
//! exotic XML features are not understood.

use serde::{Deserialize, Serialize};

/// A minimal description of a color space entry parsed from an OCIO config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcioColorSpace {
    /// The `name` attribute of the `<ColorSpace>` element.
    pub name: String,
    /// The `family` attribute, when present (used for UI grouping).
    pub family: Option<String>,
}

/// A role mapping from an OCIO config (`<Role name="..." colorspace="..."/>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcioRole {
    /// Role name (e.g. `scene_linear`, `texture`, `data`).
    pub name: String,
    /// The color space the role points at.
    pub color_space: String,
}

/// Metadata about OCIO compatibility support in this version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OcioSupport {
    /// Config scanning (color spaces, roles) is implemented.
    pub config_scanning: bool,
    /// Whether `ColorPipeline` can compile OCIO file transforms (future).
    pub file_transforms: bool,
}

/// Returns the current OCIO support level.
#[must_use]
pub const fn ocio_support() -> OcioSupport {
    OcioSupport {
        config_scanning: true,
        file_transforms: false,
    }
}

/// Extracts `key="value"` (or `key='value'`) attribute pairs from one tag
/// body. Quote-aware: values may contain spaces.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tag.len() {
        let byte = tag.as_bytes()[i];
        if byte.is_ascii_alphabetic() || byte == b'_' {
            // Attribute name: letters, digits, `_`, `-`.
            let key_start = i;
            while i < tag.len() {
                let b = tag.as_bytes()[i];
                if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' {
                    i += 1;
                } else {
                    break;
                }
            }
            let key = &tag[key_start..i];
            // `=` followed by an opening quote?
            if tag[i..].starts_with('=') {
                let after = &tag[i + 1..];
                if let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let value_start = i + 2;
                    if let Some(rel) = after[1..].find(quote) {
                        out.push((key.to_string(), after[1..1 + rel].to_string()));
                        i = value_start + rel + 1;
                        continue;
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Scans an OCIO config for its declared color spaces.
///
/// Best-effort: only self-closing/compact `<ColorSpace ...>` tags with
/// `name` attributes are recognized.
#[must_use]
pub fn scan_config_colorspaces(config: &str) -> Vec<OcioColorSpace> {
    let mut out = Vec::new();
    for segment in config.split("<ColorSpace").skip(1) {
        let Some(tag_end) = segment.find('>') else {
            continue;
        };
        let attrs = attributes(&segment[..tag_end]);
        let name = attrs
            .iter()
            .find(|(k, _)| k == "name")
            .map(|(_, v)| v.clone());
        let Some(name) = name else { continue };
        let family = attrs
            .iter()
            .find(|(k, _)| k == "family")
            .map(|(_, v)| v.clone());
        out.push(OcioColorSpace { name, family });
    }
    out
}

/// Scans an OCIO config for its role mappings.
#[must_use]
pub fn scan_config_roles(config: &str) -> Vec<OcioRole> {
    let mut out = Vec::new();
    for segment in config.split("<Role").skip(1) {
        let Some(tag_end) = segment.find('>') else {
            continue;
        };
        let attrs = attributes(&segment[..tag_end]);
        let name = attrs
            .iter()
            .find(|(k, _)| k == "name")
            .map(|(_, v)| v.clone());
        let color_space = attrs
            .iter()
            .find(|(k, _)| k == "colorspace")
            .map(|(_, v)| v.clone());
        if let (Some(name), Some(color_space)) = (name, color_space) {
            out.push(OcioRole { name, color_space });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
    <OCIOConfig>
        <Role name="scene_linear" colorspace="ACES - ACEScg"/>
        <Role name="texture" colorspace="sRGB - Texture"/>
        <ColorSpace name="ACES - ACEScg" family="ACES" isdata="false"/>
        <ColorSpace name="sRGB - Texture" family="Textures" isdata="false"/>
        <ColorSpace name="Raw" isdata="true"/>
    </OCIOConfig>
    "#;

    #[test]
    fn scans_colorspaces_with_families() {
        let spaces = scan_config_colorspaces(SAMPLE);
        assert_eq!(spaces.len(), 3);
        assert_eq!(spaces[0].name, "ACES - ACEScg");
        assert_eq!(spaces[0].family.as_deref(), Some("ACES"));
        assert_eq!(spaces[2].name, "Raw");
        assert_eq!(spaces[2].family, None);
    }

    #[test]
    fn scans_roles() {
        let roles = scan_config_roles(SAMPLE);
        assert_eq!(roles.len(), 2);
        assert_eq!(roles[0].name, "scene_linear");
        assert_eq!(roles[0].color_space, "ACES - ACEScg");
    }

    #[test]
    fn reports_support_level() {
        let support = ocio_support();
        assert!(support.config_scanning);
        assert!(
            !support.file_transforms,
            "transform compiler is future work"
        );
    }
}
