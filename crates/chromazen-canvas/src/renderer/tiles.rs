use std::collections::BTreeMap;

use super::{MAX_CANVAS_DIMENSION, history::TextureRect};

pub(crate) const TILE_SIZE: u32 = 512;
pub(crate) const TILE_BYTES: u64 = TILE_SIZE as u64 * TILE_SIZE as u64 * 4;
pub(crate) const MAX_TILE_GRID: u32 = MAX_CANVAS_DIMENSION.div_ceil(TILE_SIZE);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TileCoord {
    pub(crate) x: u32,
    pub(crate) y: u32,
}

impl TileCoord {
    pub(crate) fn containing(pixel: [u32; 2]) -> Self {
        Self {
            x: pixel[0] / TILE_SIZE,
            y: pixel[1] / TILE_SIZE,
        }
    }

    pub(crate) fn origin(self) -> [u32; 2] {
        [self.x * TILE_SIZE, self.y * TILE_SIZE]
    }
}

/// Texels beyond the document edge stay transparent because every write is clipped to the
/// document.
pub(crate) struct Tile {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
}

/// Sparse layer storage. A coordinate without a tile is fully transparent.
#[derive(Default)]
pub(crate) struct TileSet {
    tiles: BTreeMap<TileCoord, Tile>,
}

impl TileSet {
    pub(crate) fn get(&self, coord: TileCoord) -> Option<&Tile> {
        self.tiles.get(&coord)
    }

    pub(crate) fn contains(&self, coord: TileCoord) -> bool {
        self.tiles.contains_key(&coord)
    }

    pub(crate) fn insert(&mut self, coord: TileCoord, tile: Tile) -> Option<Tile> {
        self.tiles.insert(coord, tile)
    }

    pub(crate) fn get_or_insert_with(
        &mut self,
        coord: TileCoord,
        create: impl FnOnce() -> Tile,
    ) -> &Tile {
        self.tiles.entry(coord).or_insert_with(create)
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(TileCoord) -> bool) {
        self.tiles.retain(|coord, _| keep(*coord));
    }

    pub(crate) fn remove(&mut self, coord: TileCoord) -> Option<Tile> {
        self.tiles.remove(&coord)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (TileCoord, &Tile)> {
        self.tiles.iter().map(|(coord, tile)| (*coord, tile))
    }

    pub(crate) fn coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
        self.tiles.keys().copied()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub(crate) fn byte_len(&self) -> u64 {
        self.tiles.len() as u64 * TILE_BYTES
    }

    pub(crate) fn take_all(&mut self) -> BTreeMap<TileCoord, Tile> {
        std::mem::take(&mut self.tiles)
    }
}

impl From<BTreeMap<TileCoord, Tile>> for TileSet {
    fn from(tiles: BTreeMap<TileCoord, Tile>) -> Self {
        Self { tiles }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TileSpan {
    pub(crate) coord: TileCoord,
    pub(crate) document_rect: TextureRect,
}

impl TileSpan {
    pub(crate) fn local_rect(self) -> TextureRect {
        tile_local_rect(self.coord, self.document_rect)
    }
}

/// Converts a document rectangle inside `coord`'s tile to tile texture coordinates.
pub(crate) fn tile_local_rect(coord: TileCoord, rect: TextureRect) -> TextureRect {
    let [x, y] = coord.origin();
    TextureRect {
        x: rect.x - x,
        y: rect.y - y,
        ..rect
    }
}

pub(crate) fn tile_grid_size(document_size: [u32; 2]) -> [u32; 2] {
    document_size.map(|dimension| dimension.div_ceil(TILE_SIZE))
}

pub(crate) fn all_tile_coords(document_size: [u32; 2]) -> impl Iterator<Item = TileCoord> {
    let [columns, rows] = tile_grid_size(document_size);
    (0..rows).flat_map(move |y| (0..columns).map(move |x| TileCoord { x, y }))
}

pub(crate) fn tile_spans(rect: TextureRect) -> impl Iterator<Item = TileSpan> {
    let end_x = rect.x + rect.width;
    let end_y = rect.y + rect.height;
    let (columns, rows) = if rect.width == 0 || rect.height == 0 {
        (0..0, 0..0)
    } else {
        (
            rect.x / TILE_SIZE..end_x.div_ceil(TILE_SIZE),
            rect.y / TILE_SIZE..end_y.div_ceil(TILE_SIZE),
        )
    };
    rows.flat_map(move |y| {
        columns.clone().map(move |x| {
            let coord = TileCoord { x, y };
            let [tile_x, tile_y] = coord.origin();
            let left = rect.x.max(tile_x);
            let top = rect.y.max(tile_y);
            let right = end_x.min(tile_x + TILE_SIZE);
            let bottom = end_y.min(tile_y + TILE_SIZE);
            TileSpan {
                coord,
                document_rect: TextureRect {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                },
            }
        })
    })
}

/// The part of a tile that lies inside the document, in document coordinates.
pub(crate) fn tile_document_rect(coord: TileCoord, document_size: [u32; 2]) -> TextureRect {
    let [x, y] = coord.origin();
    TextureRect {
        x,
        y,
        width: document_size[0].saturating_sub(x).min(TILE_SIZE),
        height: document_size[1].saturating_sub(y).min(TILE_SIZE),
    }
}

pub(crate) fn region_has_alpha(image: &image::RgbaImage, rect: TextureRect) -> bool {
    (rect.y..rect.y + rect.height)
        .any(|y| (rect.x..rect.x + rect.width).any(|x| image.get_pixel(x, y)[3] != 0))
}

pub(crate) fn copy_tile(encoder: &mut wgpu::CommandEncoder, source: &Tile, destination: &Tile) {
    copy_texture_region(
        encoder,
        &source.texture,
        [0, 0],
        &destination.texture,
        [0, 0],
        [TILE_SIZE; 2],
    );
}

pub(crate) fn copy_texture_region(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    source_origin: [u32; 2],
    destination: &wgpu::Texture,
    destination_origin: [u32; 2],
    size: [u32; 2],
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: source_origin[0],
                y: source_origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: destination,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: destination_origin[0],
                y: destination_origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests;
