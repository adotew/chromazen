use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use crate::artwork::{ArtworkId, ReferenceManifest};

use super::references::{DecodedReference, decode_stored_reference_file};

type WakeCallback = Arc<dyn Fn() + Send + Sync>;

const LOAD_DIALOG_DELAY: Duration = Duration::from_millis(200);

pub(super) struct ReferenceLoadCompletion {
    pub(super) artwork_id: ArtworkId,
    pub(super) references: Vec<(ReferenceManifest, DecodedReference)>,
    pub(super) warnings: Vec<String>,
}

pub(super) struct ReferenceLoadController {
    completion_sender: mpsc::Sender<ReferenceLoadCompletion>,
    completion_receiver: mpsc::Receiver<ReferenceLoadCompletion>,
    wake: WakeCallback,
    active: Option<(ArtworkId, Instant)>,
}

impl ReferenceLoadController {
    pub(super) fn new(wake: WakeCallback) -> Self {
        let (completion_sender, completion_receiver) = mpsc::channel();
        Self {
            completion_sender,
            completion_receiver,
            wake,
            active: None,
        }
    }

    pub(super) fn start(
        &mut self,
        artwork_id: ArtworkId,
        sources: Vec<(ReferenceManifest, PathBuf)>,
    ) {
        self.active = Some((artwork_id.clone(), Instant::now()));
        let sender = self.completion_sender.clone();
        let wake = self.wake.clone();
        wake();
        std::thread::spawn(move || {
            let mut references = Vec::with_capacity(sources.len());
            let mut warnings = Vec::new();
            for (metadata, path) in sources {
                match decode_stored_reference_file(&path) {
                    Ok(decoded) => references.push((metadata, decoded)),
                    Err(error) => warnings.push(error),
                }
            }
            let _ = sender.send(ReferenceLoadCompletion {
                artwork_id,
                references,
                warnings,
            });
            wake();
        });
    }

    pub(super) fn take_completion(&mut self) -> Option<ReferenceLoadCompletion> {
        let completion = self.completion_receiver.try_recv().ok()?;
        if self
            .active
            .as_ref()
            .is_some_and(|(artwork_id, _)| *artwork_id == completion.artwork_id)
        {
            self.active = None;
        }
        Some(completion)
    }

    pub(super) fn is_loading(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn dialog_delay(&self) -> Option<Duration> {
        self.active
            .as_ref()
            .map(|(_, started_at)| LOAD_DIALOG_DELAY.saturating_sub(started_at.elapsed()))
    }
}
