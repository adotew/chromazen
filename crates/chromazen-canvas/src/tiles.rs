//! Sparse storage building blocks for the tiled-renderer migration.
//!
//! These do not change `Canvas`'s current storage or document limits. Pixel
//! coordinates are finite, document-relative and half-open. A missing tile is
//! transparent; a present version without resident pixels must be loaded, never
//! substituted with transparency. Residency belongs to a separate cache.

mod requests;
mod versions;

pub use requests::{
    TileReadCompletion, TileReadRequest, TileReadRequests, TileRequestError, TileRequestId,
};
pub use versions::{LayerTiles, TileBounds, TileVersion, TileVersionId};

/// Provisional until tiled GPU workloads have been measured. Stored documents
/// must record their tile size rather than relying on this default.
pub const DEFAULT_TILE_SIZE: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCoord {
    pub x: u32,
    pub y: u32,
}

/// Geometry only: constructing even a huge grid allocates no tile/pixel array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileGrid {
    document_size: [u32; 2],
    tile_size: u32,
}

impl TileGrid {
    pub fn new(document_size: [u32; 2], tile_size: u32) -> Result<Self, String> {
        if document_size.contains(&0) || tile_size == 0 {
            return Err("document and tile dimensions must be non-zero".into());
        }
        rgba_byte_len([tile_size; 2])?;
        Ok(Self {
            document_size,
            tile_size,
        })
    }

    pub fn document_size(self) -> [u32; 2] {
        self.document_size
    }

    pub fn tile_size(self) -> u32 {
        self.tile_size
    }

    pub fn dimensions(self) -> [u32; 2] {
        self.document_size.map(|size| size.div_ceil(self.tile_size))
    }

    pub fn tile_count(self) -> u64 {
        let [columns, rows] = self.dimensions();
        u64::from(columns) * u64::from(rows)
    }

    pub fn origin(self, coord: TileCoord) -> Option<[u32; 2]> {
        let origin = [
            coord.x.checked_mul(self.tile_size)?,
            coord.y.checked_mul(self.tile_size)?,
        ];
        (origin[0] < self.document_size[0] && origin[1] < self.document_size[1]).then_some(origin)
    }

    /// Edge tiles contain only these valid pixels; padding/halos are not content.
    pub fn extent(self, coord: TileCoord) -> Option<[u32; 2]> {
        let origin = self.origin(coord)?;
        Some(std::array::from_fn(|axis| {
            self.tile_size.min(self.document_size[axis] - origin[axis])
        }))
    }

    pub fn locate(self, pixel: [u32; 2]) -> Option<(TileCoord, [u32; 2])> {
        if pixel[0] >= self.document_size[0] || pixel[1] >= self.document_size[1] {
            return None;
        }
        Some((
            TileCoord {
                x: pixel[0] / self.tile_size,
                y: pixel[1] / self.tile_size,
            },
            pixel.map(|p| p % self.tile_size),
        ))
    }

    /// Lazily intersects a half-open document rectangle with tiles in row order.
    /// Signed endpoints permit off-document brush/crop regions without casting
    /// negatives to texture coordinates. Reversed/empty rectangles yield nothing.
    pub fn intersecting(self, min: [i64; 2], max: [i64; 2]) -> TileRegions {
        let min = std::array::from_fn(|axis| {
            min[axis].clamp(0, i64::from(self.document_size[axis])) as u32
        });
        let max = std::array::from_fn(|axis| {
            max[axis].clamp(0, i64::from(self.document_size[axis])) as u32
        });
        let start = min.map(|value| value / self.tile_size);
        let mut end = max.map(|value| value.div_ceil(self.tile_size));
        if min[0] >= max[0] || min[1] >= max[1] {
            end = start;
        }
        TileRegions {
            grid: self,
            min,
            max,
            start,
            end,
            next: start,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileRegion {
    pub coord: TileCoord,
    pub document_origin: [u32; 2],
    pub local_origin: [u32; 2],
    pub size: [u32; 2],
}

pub struct TileRegions {
    grid: TileGrid,
    min: [u32; 2],
    max: [u32; 2],
    start: [u32; 2],
    end: [u32; 2],
    next: [u32; 2],
}

impl Iterator for TileRegions {
    type Item = TileRegion;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next[1] >= self.end[1] {
            return None;
        }
        let coord = TileCoord {
            x: self.next[0],
            y: self.next[1],
        };
        self.next[0] += 1;
        if self.next[0] == self.end[0] {
            self.next[0] = self.start[0];
            self.next[1] += 1;
        }
        let origin = self.grid.origin(coord).expect("clipped tile coordinate");
        let extent = self.grid.extent(coord).expect("clipped tile extent");
        let document_origin = std::array::from_fn(|axis| self.min[axis].max(origin[axis]));
        Some(TileRegion {
            coord,
            document_origin,
            local_origin: std::array::from_fn(|axis| document_origin[axis] - origin[axis]),
            size: std::array::from_fn(|axis| {
                self.max[axis].min(origin[axis] + extent[axis]) - document_origin[axis]
            }),
        })
    }
}

impl std::iter::FusedIterator for TileRegions {}

pub(super) fn rgba_byte_len(extent: [u32; 2]) -> Result<u64, String> {
    if extent.contains(&0) {
        return Err("tile dimensions must be non-zero".into());
    }
    u64::from(extent[0])
        .checked_mul(u64::from(extent[1]))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "tile byte length overflows u64".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_edges_are_half_open_and_document_edges_have_partial_extents() {
        let grid = TileGrid::new([1031, 773], 512).unwrap();
        assert_eq!(grid.dimensions(), [3, 2]);
        assert_eq!(grid.tile_count(), 6);
        assert_eq!(grid.extent(TileCoord { x: 2, y: 1 }), Some([7, 261]));
        assert_eq!(
            grid.locate([512, 512]),
            Some((TileCoord { x: 1, y: 1 }, [0, 0]))
        );
        assert_eq!(grid.locate([1031, 512]), None);
        let tiles: Vec<_> = grid.intersecting([0, 0], [512, 512]).collect();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].size, [512, 512]);
        let tiles: Vec<_> = grid.intersecting([511, 511], [513, 513]).collect();
        assert_eq!(tiles.len(), 4);
        assert_eq!(tiles[0].local_origin, [511, 511]);
        assert_eq!(tiles[3].local_origin, [0, 0]);
        assert!(tiles.iter().all(|tile| tile.size == [1, 1]));
    }

    #[test]
    fn clipping_handles_negative_extreme_and_empty_regions() {
        let grid = TileGrid::new([13, 17], 8).unwrap();
        let tiles: Vec<_> = grid.intersecting([i64::MIN; 2], [i64::MAX; 2]).collect();
        assert_eq!(tiles.len(), 6);
        assert_eq!(tiles[5].document_origin, [8, 16]);
        assert_eq!(tiles[5].size, [5, 1]);
        for (min, max) in [
            ([-10, -10], [-1, -1]),
            ([17, 0], [20, 20]),
            ([2, 2], [1, 10]),
            ([0, 2], [10, 2]),
        ] {
            assert!(grid.intersecting(min, max).next().is_none());
        }
    }

    #[test]
    fn huge_grids_are_lazy_and_coordinate_multiplication_cannot_wrap() {
        let grid = TileGrid::new([u32::MAX; 2], 1).unwrap();
        assert_eq!(grid.tile_count(), u64::from(u32::MAX).pow(2));
        assert_eq!(grid.intersecting([0; 2], [i64::MAX; 2]).take(2).count(), 2);
        assert_eq!(
            grid.intersecting([i64::from(u32::MAX) - 1; 2], [i64::MAX; 2])
                .next()
                .unwrap()
                .size,
            [1, 1]
        );
        let grid = TileGrid::new([u32::MAX; 2], 512).unwrap();
        assert_eq!(grid.origin(TileCoord { x: u32::MAX, y: 0 }), None);
        assert_eq!(
            grid.extent(TileCoord {
                x: u32::MAX / 512,
                y: 0
            }),
            Some([511, 512])
        );
        assert!(TileGrid::new([1, 1], 0).is_err());
        assert!(TileGrid::new([0, 1], 512).is_err());
        assert!(TileGrid::new([1, 1], u32::MAX).is_err());
    }

    #[test]
    fn intersections_cover_each_clipped_pixel_exactly_once() {
        for side in 1..9 {
            let grid = TileGrid::new([19, 13], side).unwrap();
            for left in -2..22 {
                let min = [left, -3];
                let max = [left + 7, 12];
                let mut visits = [[0_u8; 19]; 13];
                for tile in grid.intersecting(min, max) {
                    let origin = grid.origin(tile.coord).unwrap();
                    assert_eq!(
                        tile.document_origin,
                        std::array::from_fn(|a| origin[a] + tile.local_origin[a])
                    );
                    for y in tile.document_origin[1]..tile.document_origin[1] + tile.size[1] {
                        for x in tile.document_origin[0]..tile.document_origin[0] + tile.size[0] {
                            visits[y as usize][x as usize] += 1;
                        }
                    }
                }
                for (y, row) in visits.iter().enumerate() {
                    for (x, &count) in row.iter().enumerate() {
                        assert_eq!(
                            count,
                            u8::from((x as i64) >= min[0] && (x as i64) < max[0] && y < 12)
                        );
                    }
                }
            }
        }
    }
}
