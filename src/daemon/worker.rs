//! Background export worker thread for single and batch exports.

use omastudio_engine::{
    export::{self, ExportOptions, export_photo},
    recipe::Recipe,
};

use crate::cli::{ResponseWrapper, print_json};
use super::protocol::{BatchExportItem, BatchExportItemResult, BatchExportResult, ExportResult};

pub enum ExportJob {
    Single {
        path: String,
        recipe: Box<Recipe>,
        options: Box<ExportOptions>,
        request_id: Option<u64>,
    },
    Batch {
        items: Vec<BatchExportItem>,
        options: Box<ExportOptions>,
        request_id: Option<u64>,
    },
}

pub fn spawn_export_worker() -> (std::sync::mpsc::SyncSender<ExportJob>, std::thread::JoinHandle<()>) {
    // One export at a time keeps peak RAM bounded (a 100 MP frame uses ~1.7 GB) while
    // keeping the daemon's interactive loop completely non-blocking.
    let (tx, rx) = std::sync::mpsc::sync_channel::<ExportJob>(16);
    let handle = std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            match job {
                ExportJob::Single { path, recipe, options, request_id } => {
                    match export_photo(&path, &recipe, &options) {
                        Ok(dest) => print_json(
                            &ResponseWrapper::ok_action(
                                "export",
                                ExportResult { exported_path: dest.to_string_lossy().to_string() },
                            )
                            .with_context(request_id, Some(path)),
                        ),
                        Err(e) => print_json::<()>(
                            &ResponseWrapper::err_action("export", e).with_context(request_id, Some(path)),
                        ),
                    }
                }
                ExportJob::Batch { items, options, request_id } => {
                    let item_paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
                    let name_tags = export::batch_name_tags(&item_paths);
                    let results: Vec<BatchExportItemResult> = items
                        .iter()
                        .zip(name_tags.iter())
                        .map(|(item, tag)| {
                            let recipe = item.recipe.clone().unwrap_or_else(|| Recipe::load_initial(&item.path));
                            let mut item_options = (*options).clone();
                            // The user's tag stays; the disambiguating tag is added after it
                            item_options.name_tag = match (&options.name_tag, tag) {
                                (Some(user), Some(t)) if !user.is_empty() => Some(format!("{}{}", user, t)),
                                (Some(user), _) if !user.is_empty() => Some(user.clone()),
                                (_, t) => t.clone(),
                            };
                            match export_photo(&item.path, &recipe, &item_options) {
                                Ok(dest) => BatchExportItemResult {
                                    path: item.path.clone(),
                                    success: true,
                                    exported_path: Some(dest.to_string_lossy().to_string()),
                                    error: None,
                                },
                                Err(e) => BatchExportItemResult {
                                    path: item.path.clone(),
                                    success: false,
                                    exported_path: None,
                                    error: Some(e),
                                },
                            }
                        })
                        .collect();

                    let succeeded = results.iter().filter(|r| r.success).count();
                    let failed = results.len() - succeeded;

                    print_json(
                        &ResponseWrapper::ok_action(
                            "batch_export",
                            BatchExportResult { total: results.len(), succeeded, failed, results },
                        )
                        .with_context(request_id, None),
                    );
                }
            }
        }
    });
    (tx, handle)
}
