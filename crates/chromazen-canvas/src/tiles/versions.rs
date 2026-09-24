use std::{
    collections::BTreeMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use super::{TileCoord, TileGrid, rgba_byte_len};

// IDs are process-local, not artwork IDs. Never reuse them across canvas/cache
// instances or document resets: a delayed completion must not alias new content.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_id() -> u64 {
    NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("tile identity space exhausted")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileVersionId(u64);

/// Exact tile-local alpha bounds, when known. Unknown is not empty: GPU-produced
/// versions may still need a bounded alpha reduction before transform selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileBounds {
    Unknown,
    Empty,
    NonEmpty { min: [u32; 2], max: [u32; 2] },
}

impl TileBounds {
    fn validate(self, extent: [u32; 2]) -> Result<(), String> {
        if let Self::NonEmpty { min, max } = self
            && (0..2).any(|axis| min[axis] >= max[axis] || max[axis] > extent[axis])
        {
            return Err("alpha bounds must be non-empty and inside the tile".into());
        }
        Ok(())
    }
}

/// Identity and immutable metadata for one content version. Raster edits create
/// another version; history, duplicate layers and saves share the old Arc.
///
/// `B` is a host-owned backing lease (e.g. a revision file reference or an
/// in-memory image). Its destruction releases the backing only after every
/// snapshot and I/O job has released this version. No paths/executors enter the
/// engine API. Residency is deliberately not stored in this descriptor.
pub struct TileVersion<B> {
    id: TileVersionId,
    extent: [u32; 2],
    bytes: u64,
    bounds: TileBounds,
    backing: OnceLock<B>,
}

impl<B> TileVersion<B> {
    /// The caller must retain the sole resident copy until backing is published.
    /// An unbacked version is not evictable, and read requests reject it.
    pub fn new(extent: [u32; 2], bounds: TileBounds) -> Result<Arc<Self>, String> {
        let bytes = rgba_byte_len(extent)?;
        bounds.validate(extent)?;
        Ok(Arc::new(Self {
            id: TileVersionId(next_id()),
            extent,
            bytes,
            bounds,
            backing: OnceLock::new(),
        }))
    }

    pub fn id(&self) -> TileVersionId {
        self.id
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn byte_len(&self) -> u64 {
        self.bytes
    }
    pub fn bounds(&self) -> TileBounds {
        self.bounds
    }
    pub fn backing(&self) -> Option<&B> {
        self.backing.get()
    }

    /// Publish only after an immutable backing write succeeds. A failed write
    /// leaves this unbacked and must retain its resident pixels. Attaching backing
    /// is not an artwork save and does not alter this version's content identity.
    /// Replacement is rejected so existing readers keep their original lease.
    pub fn set_backing(&self, backing: B) -> Result<(), B> {
        self.backing.set(backing)
    }
}

/// Sparse layer map with copy-on-write metadata snapshots. Cloning does not copy
/// any pixel payloads or map nodes. The first subsequent edit copies only the
/// populated index; untouched tile versions/backing leases remain shared.
pub struct LayerTiles<B> {
    grid: TileGrid,
    tiles: Arc<BTreeMap<TileCoord, Arc<TileVersion<B>>>>,
}

impl<B> Clone for LayerTiles<B> {
    fn clone(&self) -> Self {
        Self {
            grid: self.grid,
            tiles: self.tiles.clone(),
        }
    }
}

impl<B> LayerTiles<B> {
    pub fn new(grid: TileGrid) -> Self {
        Self {
            grid,
            tiles: Arc::default(),
        }
    }
    pub fn grid(&self) -> TileGrid {
        self.grid
    }
    pub fn len(&self) -> usize {
        self.tiles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
    pub fn get(&self, coord: TileCoord) -> Option<&Arc<TileVersion<B>>> {
        self.tiles.get(&coord)
    }
    pub fn iter(&self) -> impl Iterator<Item = (TileCoord, &Arc<TileVersion<B>>)> {
        self.tiles.iter().map(|(&coord, tile)| (coord, tile))
    }

    /// Logical populated pixel bytes, not resident memory. Shared versions may
    /// appear in several layers/snapshots, so do not sum this for cache budgeting.
    /// Returns `None` if the logical total cannot be represented in u64.
    pub fn logical_bytes(&self) -> Option<u64> {
        self.tiles
            .values()
            .try_fold(0_u64, |bytes, tile| bytes.checked_add(tile.byte_len()))
    }

    /// Publish a new version, or remove transparent content. Shape validation
    /// happens before modifying the map, including partial document-edge tiles.
    pub fn set(
        &mut self,
        coord: TileCoord,
        tile: Option<Arc<TileVersion<B>>>,
    ) -> Result<bool, String> {
        let extent = self
            .grid
            .extent(coord)
            .ok_or("tile coordinate is outside the document")?;
        if tile.as_ref().is_some_and(|tile| tile.extent() != extent) {
            return Err("tile extent does not match its document region".into());
        }
        let tile = tile.filter(|tile| tile.bounds() != TileBounds::Empty);
        if self.get(coord).map(|tile| tile.id()) == tile.as_ref().map(|tile| tile.id()) {
            return Ok(false);
        }
        let tiles = Arc::make_mut(&mut self.tiles);
        if let Some(tile) = tile {
            tiles.insert(coord, tile);
        } else {
            tiles.remove(&coord);
        }
        Ok(true)
    }

    /// Each changed/added coordinate, followed by removed coordinates, once each.
    /// Useful for incremental saves; removals must also update the next manifest.
    /// Document dimensions/other metadata are compared separately by the host.
    pub fn changed_coords<'a>(
        &'a self,
        previous: &'a Self,
    ) -> impl Iterator<Item = TileCoord> + 'a {
        self.tiles
            .iter()
            .filter_map(|(&coord, tile)| {
                (previous.get(coord).map(|old| old.id()) != Some(tile.id())).then_some(coord)
            })
            .chain(
                previous
                    .tiles
                    .keys()
                    .copied()
                    .filter(|coord| !self.tiles.contains_key(coord)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(extent: [u32; 2]) -> Arc<TileVersion<Vec<u8>>> {
        TileVersion::new(extent, TileBounds::Unknown).unwrap()
    }

    #[test]
    fn empty_huge_documents_have_no_pixel_grid() {
        let layer = LayerTiles::<Vec<u8>>::new(TileGrid::new([u32::MAX; 2], 512).unwrap());
        assert!(layer.is_empty());
        assert_eq!(layer.logical_bytes(), Some(0));
        assert_eq!(layer.iter().count(), 0);
    }

    #[test]
    fn logical_byte_totals_do_not_wrap_even_when_payloads_are_shared() {
        let side = u32::MAX / 2;
        let mut layer = LayerTiles::new(TileGrid::new([side * 2; 2], side).unwrap());
        let tile = version([side; 2]);
        layer
            .set(TileCoord { x: 0, y: 0 }, Some(tile.clone()))
            .unwrap();
        assert_eq!(layer.logical_bytes(), Some(tile.byte_len()));
        layer.set(TileCoord { x: 1, y: 0 }, Some(tile)).unwrap();
        assert_eq!(layer.logical_bytes(), None);
    }

    #[test]
    fn snapshots_share_versions_but_edits_and_removals_are_isolated() {
        let grid = TileGrid::new([1031, 773], 512).unwrap();
        let a = TileCoord { x: 0, y: 0 };
        let b = TileCoord { x: 2, y: 1 };
        let old = version([512; 2]);
        let mut layer = LayerTiles::new(grid);
        layer.set(a, Some(old.clone())).unwrap();
        layer.set(b, Some(version([7, 261]))).unwrap();
        let saved = layer.clone();
        assert!(Arc::ptr_eq(&layer.tiles, &saved.tiles));
        assert!(!layer.set(a, Some(old.clone())).unwrap());
        assert!(Arc::ptr_eq(&layer.tiles, &saved.tiles));
        let edited = version([512; 2]);
        layer.set(a, Some(edited.clone())).unwrap();
        layer.set(b, None).unwrap();
        assert_eq!(layer.changed_coords(&saved).collect::<Vec<_>>(), vec![a, b]);
        assert!(Arc::ptr_eq(saved.get(a).unwrap(), &old));
        assert!(saved.get(b).is_some());
        assert!(Arc::ptr_eq(layer.get(a).unwrap(), &edited));
        assert!(layer.get(b).is_none());
        // Undo/redo can exchange these maps without copying a pixel.
        let redo = layer;
        let restored = saved.clone();
        assert_eq!(restored.changed_coords(&saved).count(), 0);
        assert_eq!(redo.changed_coords(&restored).count(), 2);
    }

    #[test]
    fn transparent_tiles_are_absent_but_nonresident_versions_remain_present() {
        let coord = TileCoord { x: 0, y: 0 };
        let mut layer = LayerTiles::new(TileGrid::new([8; 2], 8).unwrap());
        let tile = version([8; 2]);
        layer.set(coord, Some(tile.clone())).unwrap();
        assert!(layer.get(coord).unwrap().backing().is_none());
        let id = tile.id();
        assert!(tile.set_backing(vec![1; 256]).is_ok());
        assert_eq!(tile.id(), id);
        assert!(tile.set_backing(vec![2; 256]).is_err());
        assert_eq!(tile.backing().unwrap()[0], 1);
        layer
            .set(
                coord,
                Some(TileVersion::new([8; 2], TileBounds::Empty).unwrap()),
            )
            .unwrap();
        assert!(layer.is_empty());
    }

    #[test]
    fn bad_extents_coordinates_and_bounds_do_not_change_a_layer() {
        let mut layer = LayerTiles::new(TileGrid::new([9; 2], 8).unwrap());
        assert!(
            layer
                .set(TileCoord { x: 1, y: 1 }, Some(version([8; 2])))
                .is_err()
        );
        assert!(layer.set(TileCoord { x: 2, y: 0 }, None).is_err());
        assert!(
            TileVersion::<()>::new(
                [8; 2],
                TileBounds::NonEmpty {
                    min: [0; 2],
                    max: [9; 2]
                }
            )
            .is_err()
        );
        assert!(
            TileVersion::<()>::new(
                [8; 2],
                TileBounds::NonEmpty {
                    min: [1; 2],
                    max: [1; 2]
                }
            )
            .is_err()
        );
        assert!(TileVersion::<()>::new([u32::MAX; 2], TileBounds::Unknown).is_err());
        assert!(layer.is_empty());
    }
}
