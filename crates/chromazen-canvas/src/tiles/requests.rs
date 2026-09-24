use std::{collections::BTreeMap, sync::Arc};

use super::{TileVersion, versions::next_id};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileRequestId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileRequestError {
    /// The only valid copy is still resident; spilling must succeed first.
    Unbacked,
    /// Retry after completions release their reservations, not by busy polling.
    BudgetExceeded,
}

/// An app-owned worker/executor reads the leased backing and returns decoded
/// premultiplied RGBA8 pixels to `TileReadRequests::complete`. Check the declared
/// extent before decoding; the reservation is for exactly that decoded payload.
/// Compressed input, decoder scratch and GPU staging need separate host budgets.
pub struct TileReadRequest<B> {
    pub id: TileRequestId,
    pub tile: Arc<TileVersion<B>>,
}

pub struct TileReadCompletion<B> {
    pub tile: Arc<TileVersion<B>>,
    /// Failure is not transparent paint and must be surfaced/retried by the host.
    pub result: Result<image::RgbaImage, String>,
}

struct PendingRead<B> {
    tile: Arc<TileVersion<B>>,
    cancelled: bool,
}

/// Bounded host request bookkeeping, independent of filesystems, threads or
/// wgpu. The caller checks a completion's version against the tile currently
/// being requested, then installs it in a separately budgeted residency cache.
/// Never write it directly into a layer's current map: that map may have changed
/// since the request began, while undo/save can still legitimately need the old
/// version. Transfer completed pixels into that cache's budget before issuing
/// more requests. Reuse this queue across document switches and call
/// `cancel_all`; replacing/dropping it would abandon outstanding reservations.
pub struct TileReadRequests<B> {
    pending: BTreeMap<TileRequestId, PendingRead<B>>,
    max_requests: usize,
    max_bytes: u64,
    reserved_bytes: u64,
}

impl<B> TileReadRequests<B> {
    pub fn new(max_requests: usize, max_bytes: u64) -> Self {
        Self {
            pending: BTreeMap::new(),
            max_requests,
            max_bytes,
            reserved_bytes: 0,
        }
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// `None` means this version already has a pending request. Requests retain
    /// both version identity and backing lifetime across document changes.
    pub fn request(
        &mut self,
        tile: &Arc<TileVersion<B>>,
    ) -> Result<Option<TileReadRequest<B>>, TileRequestError> {
        if tile.backing().is_none() {
            return Err(TileRequestError::Unbacked);
        }
        if self
            .pending
            .values()
            .any(|pending| pending.tile.id() == tile.id())
        {
            return Ok(None);
        }
        if self.pending.len() >= self.max_requests
            || tile.byte_len() > self.max_bytes - self.reserved_bytes
        {
            return Err(TileRequestError::BudgetExceeded);
        }
        let id = TileRequestId(next_id());
        self.reserved_bytes += tile.byte_len();
        self.pending.insert(
            id,
            PendingRead {
                tile: tile.clone(),
                cancelled: false,
            },
        );
        Ok(Some(TileReadRequest {
            id,
            tile: tile.clone(),
        }))
    }

    /// Obsolete jobs still hold byte reservations until they acknowledge
    /// completion/cancellation. Releasing these early would let rapid document
    /// switches schedule arbitrarily many concurrent decoder allocations.
    pub fn cancel_all(&mut self) {
        for pending in self.pending.values_mut() {
            pending.cancelled = true;
        }
    }

    /// Call once for every issued request, including failed/cancelled jobs.
    /// Unknown/duplicate/foreign/cancelled completions cannot publish pixels.
    /// A cancellation acknowledgement may use `Err` without decoding anything.
    /// The returned payload is no longer charged here: the caller must either
    /// transfer it into its cache budget or drop it before scheduling more work.
    pub fn complete(
        &mut self,
        id: TileRequestId,
        result: Result<image::RgbaImage, String>,
    ) -> Option<TileReadCompletion<B>> {
        let pending = self.pending.remove(&id)?;
        self.reserved_bytes -= pending.tile.byte_len();
        if pending.cancelled {
            return None;
        }
        let result = result.and_then(|image| {
            if [image.width(), image.height()] == pending.tile.extent() {
                Ok(image)
            } else {
                Err("loaded tile extent does not match its version".into())
            }
        });
        Some(TileReadCompletion {
            tile: pending.tile,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiles::{LayerTiles, TileBounds, TileCoord, TileGrid};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn backed() -> Arc<TileVersion<image::RgbaImage>> {
        let tile = TileVersion::new([4; 2], TileBounds::Unknown).unwrap();
        tile.set_backing(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([128, 0, 0, 128]),
        ))
        .unwrap();
        tile
    }

    // The same contract works with a synchronous test host, native workers or a
    // browser promise. Only this host knows how to interpret its backing lease.
    fn memory_host(
        request: &TileReadRequest<image::RgbaImage>,
    ) -> Result<image::RgbaImage, String> {
        Ok(request.tile.backing().unwrap().clone())
    }

    #[test]
    fn requests_are_deduplicated_and_limited_by_count_and_bytes() {
        let a = backed();
        let b = backed();
        let mut reads = TileReadRequests::new(2, 64);
        let request = reads.request(&a).unwrap().unwrap();
        assert!(reads.request(&a).unwrap().is_none());
        assert!(matches!(
            reads.request(&b),
            Err(TileRequestError::BudgetExceeded)
        ));
        assert_eq!(reads.pending_count(), 1);
        assert_eq!(reads.reserved_bytes(), 64);
        let completed = reads.complete(request.id, memory_host(&request)).unwrap();
        assert_eq!(completed.tile.id(), a.id());
        assert_eq!(
            completed.result.unwrap().get_pixel(0, 0).0,
            [128, 0, 0, 128]
        );
        assert_eq!(reads.reserved_bytes(), 0);
        assert!(reads.complete(request.id, memory_host(&request)).is_none());
        assert!(reads.request(&b).unwrap().is_some());
        let mut count_limited = TileReadRequests::new(1, 1024);
        count_limited.request(&a).unwrap();
        assert!(matches!(
            count_limited.request(&b),
            Err(TileRequestError::BudgetExceeded)
        ));
    }

    #[test]
    fn cancellation_keeps_reservations_until_old_jobs_finish() {
        let a = backed();
        let b = backed();
        let mut reads = TileReadRequests::new(1, 64);
        let stale = reads.request(&a).unwrap().unwrap();
        reads.cancel_all();
        assert_eq!(reads.reserved_bytes(), 64);
        assert!(matches!(
            reads.request(&b),
            Err(TileRequestError::BudgetExceeded)
        ));
        assert!(reads.complete(stale.id, memory_host(&stale)).is_none());
        let current = reads.request(&b).unwrap().unwrap();
        assert_ne!(stale.id, current.id);
        assert!(reads.complete(stale.id, memory_host(&stale)).is_none());
        assert_eq!(reads.reserved_bytes(), 64);
        assert!(
            reads
                .complete(current.id, memory_host(&current))
                .unwrap()
                .result
                .is_ok()
        );
    }

    #[test]
    fn foreign_sessions_failures_and_bad_dimensions_cannot_publish_pixels() {
        let a = backed();
        let mut first = TileReadRequests::new(1, 64);
        let mut second = TileReadRequests::new(1, 64);
        let foreign = first.request(&a).unwrap().unwrap();
        let local = second.request(&a).unwrap().unwrap();
        assert_ne!(foreign.id, local.id);
        assert!(second.complete(foreign.id, memory_host(&foreign)).is_none());
        assert_eq!(second.pending_count(), 1);
        assert!(
            second
                .complete(local.id, Ok(image::RgbaImage::new(5, 4)))
                .unwrap()
                .result
                .is_err()
        );
        let retry = second.request(&a).unwrap().unwrap();
        assert!(
            second
                .complete(retry.id, Err("missing tile file".into()))
                .unwrap()
                .result
                .is_err()
        );
        assert_eq!(second.reserved_bytes(), 0);
    }

    #[test]
    fn loading_an_old_version_does_not_replace_current_edits() {
        let coord = TileCoord { x: 0, y: 0 };
        let mut layer = LayerTiles::new(TileGrid::new([4; 2], 4).unwrap());
        let old = backed();
        layer.set(coord, Some(old.clone())).unwrap();
        let snapshot = layer.clone();
        let mut reads = TileReadRequests::new(1, 64);
        let request = reads.request(&old).unwrap().unwrap();
        let edited = backed();
        layer.set(coord, Some(edited.clone())).unwrap();
        let completion = reads.complete(request.id, memory_host(&request)).unwrap();
        assert_eq!(completion.tile.id(), snapshot.get(coord).unwrap().id());
        assert_ne!(completion.tile.id(), layer.get(coord).unwrap().id());
        assert_eq!(layer.get(coord).unwrap().id(), edited.id());
    }

    #[test]
    fn failed_spill_remains_unbacked_and_cannot_be_read() {
        let tile = TileVersion::<()>::new([4; 2], TileBounds::Unknown).unwrap();
        let mut reads = TileReadRequests::new(1, 64);
        assert!(matches!(
            reads.request(&tile),
            Err(TileRequestError::Unbacked)
        ));
        assert_eq!(reads.reserved_bytes(), 0);
        // Simulate a successful retry. The content ID remains unchanged.
        tile.set_backing(()).unwrap();
        assert!(reads.request(&tile).unwrap().is_some());
    }

    #[test]
    fn snapshots_and_in_flight_jobs_keep_backing_alive() {
        struct Lease(Arc<AtomicUsize>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let released = Arc::new(AtomicUsize::new(0));
        let mut layer = LayerTiles::new(TileGrid::new([4; 2], 4).unwrap());
        let tile = TileVersion::new([4; 2], TileBounds::Unknown).unwrap();
        assert!(tile.set_backing(Lease(released.clone())).is_ok());
        layer
            .set(TileCoord { x: 0, y: 0 }, Some(tile.clone()))
            .unwrap();
        let snapshot = layer.clone();
        let mut reads = TileReadRequests::new(1, 64);
        let request = reads.request(&tile).unwrap().unwrap();
        drop(tile);
        drop(layer);
        assert_eq!(released.load(Ordering::Relaxed), 0);
        drop(snapshot);
        assert_eq!(released.load(Ordering::Relaxed), 0);
        reads.cancel_all();
        assert!(
            reads
                .complete(request.id, Err("cancelled".into()))
                .is_none()
        );
        assert_eq!(released.load(Ordering::Relaxed), 0);
        drop(request);
        assert_eq!(released.load(Ordering::Relaxed), 1);
    }
}
