//! JSON-lines daemon request and response types.

use std::sync::atomic::AtomicUsize;

use omastudio_engine::{export::ExportOptions, icc, pipeline, preview_shm::PublishedFrame, recipe::Recipe};
use serde::{Deserialize, Serialize};

/// Alternates the compare view between two files, so it never reads one being written.
pub static REFERENCE_SLOT: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize)]
pub struct RenderResult {
    pub image: String,
    pub histogram: pipeline::histogram::HistogramData,
    /// Why the soft proof could not be applied (the preview is then shown unproofed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stride: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_space: Option<&'static str>,
    /// Preview transport (`"dma-buf"` or `"memfd"`), omitted for legacy file-backed responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<&'static str>,
}

impl RenderResult {
    pub fn from_file(
        image: String,
        histogram: pipeline::histogram::HistogramData,
        proof_error: Option<String>,
    ) -> Self {
        Self {
            image,
            histogram,
            proof_error,
            slot: None,
            generation: None,
            width: None,
            height: None,
            stride: None,
            format: None,
            color_space: None,
            transport: None,
        }
    }

    pub fn from_published(
        frame: PublishedFrame,
        histogram: pipeline::histogram::HistogramData,
        proof_error: Option<String>,
    ) -> Self {
        Self {
            image: frame.url,
            histogram,
            proof_error,
            slot: Some(frame.slot),
            generation: Some(frame.generation),
            width: Some(frame.width),
            height: Some(frame.height),
            stride: Some(frame.stride),
            format: Some(frame.format.as_str()),
            color_space: Some(frame.color_space.as_str()),
            transport: Some("memfd"),
        }
    }

    pub fn from_dmabuf(
        desc: &omastudio_engine::preview_shm::DmaBufFrameDesc,
        histogram: pipeline::histogram::HistogramData,
        proof_error: Option<String>,
    ) -> Self {
        Self {
            image: format!("dmabuf://omastudio/preview/{}", desc.generation),
            histogram,
            proof_error,
            slot: None,
            generation: Some(desc.generation),
            width: Some(desc.width),
            height: Some(desc.height),
            stride: Some(desc.stride),
            format: Some("rgba16f"),
            color_space: Some(desc.color_space.as_str()),
            transport: Some("dma-buf"),
        }
    }
}

/// Soft-proofing settings sent with `adjust` and `detail`: a view option, not part of the edit.
#[derive(Deserialize)]
pub struct ProofRequest {
    pub profile: String,
    #[serde(default)]
    pub intent: icc::RenderingIntent,
    #[serde(default)]
    pub gamut_warning: bool,
}

/// Applies the soft proof, if one was asked for, to an 8-bit sRGB preview.
pub fn apply_proof(rgb: &mut [u8], proof: Option<&ProofRequest>) -> Option<String> {
    let proof = proof?;
    icc::proof_profile_bytes(&proof.profile)
        .and_then(|bytes| icc::soft_proof_rgb8(rgb, &bytes, proof.intent, proof.gamut_warning))
        .err()
}

#[derive(Serialize)]
pub struct ExportResult {
    pub exported_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExportItem {
    pub path: String,
    #[serde(default)]
    pub recipe: Option<Recipe>,
}

#[derive(Serialize)]
pub struct BatchExportItemResult {
    pub path: String,
    pub success: bool,
    pub exported_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct BatchExportResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub results: Vec<BatchExportItemResult>,
}

#[derive(Serialize)]
pub struct FullReady {
    pub path: String,
    pub width: usize,
    pub height: usize,
}

#[derive(Serialize)]
pub struct DetailResult {
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_error: Option<String>,
    /// The rendered part of the photo, normalised: x, y, width, height.
    pub region: [f64; 4],
    pub width: usize,
    pub height: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stride: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_space: Option<&'static str>,
}

#[derive(Deserialize)]
pub struct DaemonCommand {
    pub cmd: String,
    #[serde(default)]
    pub request_id: Option<u64>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub recipe: Option<Recipe>,
    #[serde(default)]
    pub split: Option<f32>,
    #[serde(default)]
    pub out: Option<String>,
    #[serde(default)]
    pub options: Option<ExportOptions>,
    #[serde(default)]
    pub highlight_mask: Option<bool>,
    #[serde(default)]
    pub shadow_mask: Option<bool>,
    #[serde(default)]
    pub items: Option<Vec<BatchExportItem>>,
    #[serde(default)]
    pub rating: Option<u8>,
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub color_label: Option<String>,
    #[serde(default)]
    pub search_query: Option<String>,
    /// `detail`: normalised x, y, width, height of the photo to render.
    #[serde(default)]
    pub region: Option<[f64; 4]>,
    /// `detail`: size of the rendered image in device pixels.
    #[serde(default)]
    pub out_width: Option<u32>,
    #[serde(default)]
    pub out_height: Option<u32>,
    /// `adjust`, `detail`: soft-proof the preview against an output profile
    #[serde(default)]
    pub proof: Option<ProofRequest>,
    /// `adjust`, `detail`: mark in-focus edges (see `pipeline::peaking`)
    #[serde(default)]
    pub focus_peaking: Option<bool>,
    /// `detail`: "loupe" renders for the loupe, into files of its own, and is echoed back
    #[serde(default)]
    pub tag: Option<String>,
    /// `adjust`: request `memfd` transport (used during colour picking, compare, or DMA-BUF fallback)
    #[serde(default)]
    pub prefer_memfd: Option<bool>,
    /// `adjust`, `reference`: active display colour space reported by Wayland (`"srgb"`, `"display_p3"`, `"bt2020"`, `"bt2100_pq"`, `"bt2100_hlg"`)
    #[serde(default)]
    pub display_color_space: Option<String>,
}
