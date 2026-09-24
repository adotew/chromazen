use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Payload sizes owned by the canvas, not a driver VRAM measurement. Excludes
/// allocator padding, pipelines/bind groups, caller-owned images and resources
/// retained only by submitted GPU commands. Query after GPU completion for a
/// useful steady-state baseline; do not use this as an allocation budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanvasMemoryUsage {
    /// Live layer pixels, thumbnails and settings buffers.
    pub layers: u64,
    /// Resident before-image versions and detached undo/redo layers.
    pub history: u64,
    /// Smudge, clipping, stroke/preview masks and window backdrop.
    pub scratch: u64,
    pub brush: u64,
    pub buffers: u64,
    /// Allocated CPU stamp/preview queue capacity, not just queued items.
    pub stamp_queues: u64,
    /// Padded layer readback buffers, including readbacks owned by callers.
    pub readbacks: u64,
}

impl CanvasMemoryUsage {
    pub fn gpu_payload_bytes(self) -> u64 {
        self.layers + self.history + self.scratch + self.brush + self.buffers + self.readbacks
    }
}

/// Lifetime counters, unaffected by undo or replacing the document. These count
/// encoded painting work, not GPU execution time or display/thumbnail passes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanvasWork {
    pub committed_dabs: u64,
    /// Brush/eraser instances after subdivision at tile boundaries.
    pub tile_fragments: u64,
    pub preview_dabs: u64,
    /// Mask, smudge, stroke commit and mask-clear passes.
    pub paint_passes: u64,
    /// Layer/brush pixel and stamp uploads; excludes small uniform updates.
    pub upload_bytes: u64,
    /// Layer readback bytes, including row padding.
    pub readback_bytes: u64,
}

pub(super) fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    // All canvas-owned textures are single-mip, single-sample 2D textures.
    debug_assert_eq!(texture.mip_level_count(), 1);
    debug_assert_eq!(texture.sample_count(), 1);
    let (block_width, block_height) = texture.format().block_dimensions();
    u64::from(texture.width().div_ceil(block_width))
        * u64::from(texture.height().div_ceil(block_height))
        * u64::from(
            texture
                .format()
                .block_copy_size(None)
                .expect("color texture"),
        )
}

#[derive(Default)]
pub(super) struct ReadbackTracker {
    live: AtomicU64,
    submitted: AtomicU64,
}

impl ReadbackTracker {
    pub(super) fn live(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    pub(super) fn submitted(&self) -> u64 {
        self.submitted.load(Ordering::Relaxed)
    }

    pub(super) fn retain(self: &Arc<Self>, bytes: u64) -> Arc<ReadbackLease> {
        self.live.fetch_add(bytes, Ordering::Relaxed);
        self.lease(bytes)
    }

    pub(super) fn try_retain(
        self: &Arc<Self>,
        bytes: u64,
        budget: u64,
    ) -> Option<Arc<ReadbackLease>> {
        self.live
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
                live.checked_add(bytes).filter(|total| *total <= budget)
            })
            .ok()?;
        Some(self.lease(bytes))
    }

    fn lease(self: &Arc<Self>, bytes: u64) -> Arc<ReadbackLease> {
        self.submitted.fetch_add(bytes, Ordering::Relaxed);
        Arc::new(ReadbackLease {
            tracker: self.clone(),
            bytes,
        })
    }
}

// Both the readback owner and mapping callbacks retain this lease. Dropping a
// readback before mapping completes must not report its buffers as freed yet.
pub(super) struct ReadbackLease {
    tracker: Arc<ReadbackTracker>,
    bytes: u64,
}

impl Drop for ReadbackLease {
    fn drop(&mut self) {
        self.tracker.live.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_reservations_cannot_exceed_the_readback_budget() {
        let tracker = Arc::new(ReadbackTracker::default());
        let workers: Vec<_> = (0..16)
            .map(|_| {
                let tracker = tracker.clone();
                std::thread::spawn(move || tracker.try_retain(64, 64))
            })
            .collect();
        let leases: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(leases.iter().filter(|lease| lease.is_some()).count(), 1);
        assert_eq!(tracker.live(), 64);
        assert_eq!(tracker.submitted(), 64);
        drop(leases);
        assert_eq!(tracker.live(), 0);
    }

    #[test]
    fn readback_bytes_remain_live_until_both_owner_and_callback_release_them() {
        let tracker = Arc::new(ReadbackTracker::default());
        let owner = tracker.retain(512);
        let callback = owner.clone();
        let other = tracker.retain(256);
        drop(owner);
        assert_eq!(tracker.live(), 768);
        drop(callback);
        assert_eq!(tracker.live(), 256);
        drop(other);
        assert_eq!(tracker.live(), 0);
        assert_eq!(tracker.submitted(), 768);
    }
}
