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

use crate::color_space::ColorSpace;
use crate::hdr::ToneMapper;
use crate::transfer::TransferFunction;
use crate::ColorPipeline;

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

/// Heuristically maps an OCIO color-space name (or role target) onto the
/// engine's known color spaces and transfer functions.
///
/// This is name matching, **not** transform compilation: it recognizes the
/// common industry naming conventions (`ACEScg`, `sRGB`, `Rec.709`,
/// `Rec.2020`, `PQ`/`HDR10`, `HLG`, `P3`/`DCI`, `linear`) so a scanned OCIO
/// config can drive a [`ColorPipeline`] without a full OCIO runtime. Space
/// identification ignores separators and case; first match wins.
#[must_use]
pub fn color_space_for_name(name: &str) -> Option<(ColorSpace, TransferFunction)> {
    let n = name.to_ascii_lowercase();
    let n = n.replace([' ', '-', '.', '_'], "");

    // More specific patterns first.
    if n.contains("acescg") {
        return Some((ColorSpace::Aces, TransferFunction::Linear));
    }
    if n.contains("aces2065") || n == "aces" || n.contains("ap0") {
        return Some((ColorSpace::Aces, TransferFunction::Linear));
    }
    if n.contains("rec2020") || n.contains("bt2020") || n.contains("2020") {
        let tf = if n.contains("pq") || n.contains("hdr10") {
            TransferFunction::Pq
        } else if n.contains("hlg") {
            TransferFunction::Hlg
        } else {
            TransferFunction::Linear
        };
        return Some((ColorSpace::Rec2020, tf));
    }
    if n.contains("p3") || n.contains("dci") {
        let tf = if n.contains("pq") {
            TransferFunction::Pq
        } else {
            TransferFunction::Gamma(2.6)
        };
        return Some((ColorSpace::DciP3, tf));
    }
    if n.contains("srgb") || n.contains("texture") || n.contains("display") {
        return Some((ColorSpace::Srgb, TransferFunction::Srgb));
    }
    if n.contains("rec709") || n.contains("bt709") || n.contains("709") {
        return Some((ColorSpace::Rec709, TransferFunction::Gamma(2.4)));
    }
    if n.contains("linear") || n.contains("raw") || n.contains("data") {
        return Some((ColorSpace::Linear, TransferFunction::Linear));
    }
    None
}

/// Builds an sRGB display pipeline from a scanned OCIO color-space name:
/// decodes that space, converts gamut, tone maps when the input is HDR, and
/// encodes to sRGB. Returns `None` when the name is not recognized.
///
/// ```
/// use tpt_av_visual_color::ocio::display_pipeline_for;
///
/// let pipeline = display_pipeline_for("ACES - ACEScg").expect("known name");
/// // Render with `pipeline` to convert ACEScg scene data to an sRGB display.
/// ```
#[must_use]
pub fn display_pipeline_for(name: &str) -> Option<ColorPipeline> {
    let (space, transfer) = color_space_for_name(name)?;
    let mut pipeline =
        ColorPipeline::new(space, transfer, ColorSpace::Srgb, TransferFunction::Srgb);
    if matches!(transfer, TransferFunction::Pq | TransferFunction::Hlg) {
        pipeline = pipeline
            .with_input_linear_scale(1.0 / 0.0203)
            .with_tone_mapper(ToneMapper::AcesFilmic);
    }
    Some(pipeline)
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn maps_common_industry_names() {
        let (space, tf) = color_space_for_name("ACES - ACEScg").expect("acescg");
        assert_eq!(space, ColorSpace::Aces);
        assert_eq!(tf, TransferFunction::Linear);

        let (space, tf) = color_space_for_name("sRGB - Texture").expect("srgb");
        assert_eq!(space, ColorSpace::Srgb);
        assert_eq!(tf, TransferFunction::Srgb);

        let (space, tf) = color_space_for_name("Rec.2020 HDR10 PQ").expect("2020 pq");
        assert_eq!(space, ColorSpace::Rec2020);
        assert_eq!(tf, TransferFunction::Pq);
    }

    #[test]
    fn unknown_names_return_none() {
        assert!(color_space_for_name("Studio Camera A").is_none());
        assert!(color_space_for_name("").is_none());
    }

    #[test]
    fn display_pipeline_handles_hdr() {
        let p = display_pipeline_for("Rec.2020 HDR10 PQ").expect("known");
        assert!(p.tone_mapper.is_some(), "PQ input needs tone mapping");
        assert!((p.input_linear_scale - 1.0 / 0.0203).abs() < 1e-6);
        assert_eq!(p.output_transfer, TransferFunction::Srgb);
    }

    #[test]
    fn display_pipeline_srgb_is_unity() {
        let p = display_pipeline_for("sRGB - Display").expect("known");
        assert_eq!(p.input_transfer, TransferFunction::Srgb);
        assert!(p.tone_mapper.is_none());
    }
}
