//! JSON-lines daemon request and response types.

use std::sync::atomic::AtomicUsize;

use omastudio_engine::{export::ExportOptions, icc, pipeline, recipe::Recipe};
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
}
