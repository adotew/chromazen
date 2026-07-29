use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
};

use super::references::{DecodedReference, decode_reference_file};

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

pub(super) struct ReferenceImportCompletion {
    pub(super) placement: Option<[f32; 2]>,
    pub(super) images: Vec<DecodedReference>,
    pub(super) errors: Vec<String>,
}

pub(super) struct ReferenceImportController {
    completion_sender: mpsc::Sender<ReferenceImportCompletion>,
    completion_receiver: mpsc::Receiver<ReferenceImportCompletion>,
    wake: WakeCallback,
    in_flight: usize,
}

impl ReferenceImportController {
    pub(super) fn new(wake: WakeCallback) -> Self {
        let (completion_sender, completion_receiver) = mpsc::channel();
        Self {
            completion_sender,
            completion_receiver,
            wake,
            in_flight: 0,
        }
    }

    pub(super) fn start(&mut self, paths: Vec<PathBuf>, placement: Option<[f32; 2]>) {
        if paths.is_empty() {
            return;
        }
        self.in_flight = self.in_flight.saturating_add(1);
        let sender = self.completion_sender.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let mut images = Vec::new();
            let mut errors = Vec::new();
            for path in paths {
                match decode_reference_file(&path) {
                    Ok(image) => images.push(image),
                    Err(error) => errors.push(error),
                }
            }
            let _ = sender.send(ReferenceImportCompletion {
                placement,
                images,
                errors,
            });
            wake();
        });
    }

    pub(super) fn take_completion(&mut self) -> Option<ReferenceImportCompletion> {
        let completion = self.completion_receiver.try_recv().ok()?;
        self.in_flight = self.in_flight.saturating_sub(1);
        Some(completion)
    }
}

pub(super) fn choose_reference_paths() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Reference images", &["png", "jpg", "jpeg"])
        .pick_files()
        .unwrap_or_default()
}
