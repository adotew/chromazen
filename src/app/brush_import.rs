use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use crate::{config::ConfigStore, paint::PaintTool};

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

const IMPORT_DIALOG_DELAY: Duration = Duration::from_millis(200);

pub(super) struct BrushImportCompletion {
    pub(super) tool: PaintTool,
    pub(super) imported_ids: Vec<String>,
    pub(super) warnings: Vec<String>,
    pub(super) errors: Vec<String>,
}

pub(super) struct BrushImportController {
    completion_sender: mpsc::Sender<BrushImportCompletion>,
    completion_receiver: mpsc::Receiver<BrushImportCompletion>,
    wake: WakeCallback,
    started_at: Option<Instant>,
}

impl BrushImportController {
    pub(super) fn new(wake: WakeCallback) -> Self {
        let (completion_sender, completion_receiver) = mpsc::channel();
        Self {
            completion_sender,
            completion_receiver,
            wake,
            started_at: None,
        }
    }

    pub(super) fn start(&mut self, tool: PaintTool, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.started_at.is_some() {
            return;
        }
        self.started_at = Some(Instant::now());
        let sender = self.completion_sender.clone();
        let wake = self.wake.clone();
        wake();
        std::thread::spawn(move || {
            let mut imported_ids = Vec::new();
            let mut warnings = Vec::new();
            let mut errors = Vec::new();
            match ConfigStore::discover() {
                Ok(store) => {
                    for path in paths {
                        match store.import_abr(&path) {
                            Ok(result) => {
                                imported_ids.extend(result.imported_ids);
                                warnings.extend(result.warnings);
                            }
                            Err(error) => errors.push(error.to_string()),
                        }
                    }
                }
                Err(error) => errors.push(error.to_string()),
            }
            let _ = sender.send(BrushImportCompletion {
                tool,
                imported_ids,
                warnings,
                errors,
            });
            wake();
        });
    }

    pub(super) fn take_completion(&mut self) -> Option<BrushImportCompletion> {
        let completion = self.completion_receiver.try_recv().ok()?;
        self.started_at = None;
        Some(completion)
    }

    pub(super) fn dialog_delay(&self) -> Option<Duration> {
        self.started_at
            .map(|started_at| IMPORT_DIALOG_DELAY.saturating_sub(started_at.elapsed()))
    }
}

pub(super) fn choose_abr_paths() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Photoshop brushes", &["abr"])
        .pick_files()
        .unwrap_or_default()
}
