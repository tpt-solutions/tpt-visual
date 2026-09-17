//! OpenColorIO config compatibility.
//!
//! What exists today is a **best-effort config scanner** that extracts the
//! color spaces, displays, and roles declared in a `.ocio` (XML) config so
//! host applications can enumerate a user's OCIO environment and map it onto
//! [`crate::ColorPipeline`] inputs, plus a **single-transform compiler**:
//! when a `<ColorSpace>` entry's transform is a bare `<FileTransform src=".."/>`
//! pointing at a `.cube` LUT, that LUT is loaded and attached to the compiled
//! pipeline. Attribute parsing is deliberately minimal (key="value"
//! scanning) — OCIO configs that rely on YAML syntax or exotic XML features
//! are not understood. Multi-step transform graphs (`GroupTransform`,
//! `MatrixTransform`, `ExponentTransform`, `CDLTransform`, chained
//! `ColorSpaceTransform` references) are not compiled; only the single
//! `FileTransform` case is, since it maps directly onto the pipeline's
//! existing LUT slot.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::color_space::ColorSpace;
use crate::hdr::ToneMapper;
use crate::luts::{Cube, Lut3D};
use crate::transfer::TransferFunction;
use crate::{ColorPipeline, Result, VisualError};

/// A minimal description of a color space entry parsed from an OCIO config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcioColorSpace {
    /// The `name` attribute of the `<ColorSpace>` element.
    pub name: String,
    /// The `family` attribute, when present (used for UI grouping).
    pub family: Option<String>,
    /// The `src` attribute of a nested `<FileTransform src="..."/>`, when
    /// this color space's transform is a bare file transform.
    pub file_transform_src: Option<String>,
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
    /// Whether `ColorPipeline` can compile a color space's `FileTransform`
    /// (a bare reference to a `.cube` LUT) into the pipeline's LUT slot.
    /// Multi-step transform graphs are not compiled.
    pub file_transforms: bool,
}

/// Returns the current OCIO support level.
#[must_use]
pub const fn ocio_support() -> OcioSupport {
    OcioSupport {
        config_scanning: true,
        file_transforms: true,
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
        // The body of this element runs until the next `<ColorSpace` (the
        // split boundary) or the end of the config — either way, a nested
        // `<FileTransform>` for *this* color space lives in `segment` past
        // `tag_end`.
        let file_transform_src = segment[tag_end..].find("<FileTransform").and_then(|rel| {
            let ft = &segment[tag_end + rel + "<FileTransform".len()..];
            let ft_end = ft.find('>')?;
            attributes(&ft[..ft_end])
                .into_iter()
                .find(|(k, _)| k == "src")
                .map(|(_, v)| v)
        });
        out.push(OcioColorSpace {
            name,
            family,
            file_transform_src,
        });
    }
    out
}

/// Finds a scanned color space by name and, if its transform is a bare
/// `<FileTransform src="..."/>` referencing a `.cube` file, loads and
/// returns that LUT. `search_dir` is resolved against `src` the way OCIO
/// resolves a config's `search_path` (relative to the config's own
/// directory).
///
/// Returns `Ok(None)` when the color space is not found or has no
/// (recognized) file transform — this is not an error, since most color
/// spaces in a real config describe matrix/exponent transforms this
/// compiler does not attempt. Returns `Err` when a file transform is
/// present but the referenced file cannot be read/parsed, or resolves to a
/// 1D rather than 3D LUT (`ColorPipeline` only carries a 3D LUT slot).
pub fn compile_file_transform_lut(
    config: &str,
    search_dir: &Path,
    name: &str,
) -> Result<Option<Lut3D>> {
    let Some(space) = scan_config_colorspaces(config)
        .into_iter()
        .find(|cs| cs.name == name)
    else {
        return Ok(None);
    };
    let Some(src) = space.file_transform_src else {
        return Ok(None);
    };
    let path: PathBuf = search_dir.join(&src);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        VisualError::InvalidOperation(format!(
            "OCIO FileTransform for \"{name}\" references \"{}\" ({}): {e}",
            path.display(),
            src
        ))
    })?;
    match crate::luts::parse_cube(&text)? {
        Cube::Lut3D(lut) => Ok(Some(lut)),
        Cube::Lut1D(_) => Err(VisualError::InvalidOperation(format!(
            "OCIO FileTransform for \"{name}\" resolves to a 1D LUT (\"{src}\"); \
             ColorPipeline only carries a 3D LUT"
        ))),
    }
}

/// Compiles a full display pipeline for a named color space from a scanned
/// OCIO config: maps the name to an engine color space/transfer (as
/// [`display_pipeline_for`]) and, when that color space declares a bare
/// `FileTransform`, loads and attaches the referenced `.cube` LUT.
///
/// This is still name-mapping plus a single optional LUT stage, not a
/// general OCIO transform interpreter — see the module docs.
pub fn compile_display_pipeline(
    config: &str,
    search_dir: &Path,
    name: &str,
) -> Result<Option<ColorPipeline>> {
    let Some(mut pipeline) = display_pipeline_for(name) else {
        return Ok(None);
    };
    if let Some(lut) = compile_file_transform_lut(config, search_dir, name)? {
        pipeline = pipeline.with_lut(lut);
    }
    Ok(Some(pipeline))
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
        assert_eq!(spaces[0].file_transform_src, None);
    }

    const SAMPLE_WITH_FILE_TRANSFORM: &str = r#"
    <OCIOConfig>
        <ColorSpace name="Look - Teal Orange" family="Look">
            <FileTransform src="teal_orange.cube" interpolation="linear"/>
        </ColorSpace>
        <ColorSpace name="Raw" isdata="true"/>
    </OCIOConfig>
    "#;

    #[test]
    fn scans_nested_file_transform_src() {
        let spaces = scan_config_colorspaces(SAMPLE_WITH_FILE_TRANSFORM);
        assert_eq!(spaces[0].name, "Look - Teal Orange");
        assert_eq!(
            spaces[0].file_transform_src.as_deref(),
            Some("teal_orange.cube")
        );
        assert_eq!(spaces[1].file_transform_src, None);
    }

    #[test]
    fn compile_file_transform_lut_loads_referenced_cube() {
        let dir = std::env::temp_dir().join(format!(
            "tpt-ocio-test-{}-{}",
            std::process::id(),
            "compile_file_transform_lut_loads_referenced_cube"
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cube_path = dir.join("teal_orange.cube");
        std::fs::write(
            &cube_path,
            "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
        )
        .unwrap();

        let lut =
            compile_file_transform_lut(SAMPLE_WITH_FILE_TRANSFORM, &dir, "Look - Teal Orange")
                .unwrap()
                .expect("file transform LUT");
        assert_eq!(lut.size, 2);
        assert_eq!(lut.sample([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);

        // A color space with no FileTransform compiles to `None`, not an error.
        assert!(
            compile_file_transform_lut(SAMPLE_WITH_FILE_TRANSFORM, &dir, "Raw")
                .unwrap()
                .is_none()
        );
        // An unknown name also compiles to `None`.
        assert!(
            compile_file_transform_lut(SAMPLE_WITH_FILE_TRANSFORM, &dir, "Nope")
                .unwrap()
                .is_none()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compile_file_transform_lut_errors_on_missing_file() {
        let dir = std::env::temp_dir();
        let err =
            compile_file_transform_lut(SAMPLE_WITH_FILE_TRANSFORM, &dir, "Look - Teal Orange")
                .unwrap_err();
        assert!(err.to_string().contains("teal_orange.cube"));
    }

    #[test]
    fn compile_display_pipeline_attaches_lut_and_maps_name() {
        let dir = std::env::temp_dir().join(format!(
            "tpt-ocio-test-{}-{}",
            std::process::id(),
            "compile_display_pipeline_attaches_lut_and_maps_name"
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("teal_orange.cube"),
            "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
        )
        .unwrap();

        // Name mapping only recognizes industry-standard tokens; a "Look"
        // family name has none, so no base color space is inferred and the
        // whole compile is `None` even though the LUT file exists.
        assert!(
            compile_display_pipeline(SAMPLE_WITH_FILE_TRANSFORM, &dir, "Look - Teal Orange")
                .unwrap()
                .is_none()
        );

        let pipeline = compile_display_pipeline(SAMPLE, &dir, "ACES - ACEScg")
            .unwrap()
            .expect("ACES maps to a known space");
        assert_eq!(pipeline.input_space, ColorSpace::Aces);
        assert!(pipeline.lut.is_none(), "SAMPLE has no FileTransform");

        std::fs::remove_dir_all(&dir).ok();
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
            support.file_transforms,
            "single FileTransform LUT compilation is implemented"
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
