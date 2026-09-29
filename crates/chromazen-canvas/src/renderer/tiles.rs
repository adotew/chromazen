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

/// The part of a document rectangle that falls inside one tile.
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
mod tests {
    use super::*;

    fn rect(x: u32, y: u32, width: u32, height: u32) -> TextureRect {
        TextureRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn grid_rounds_partial_tiles_up() {
        assert_eq!(tile_grid_size([4000, 4000]), [8, 8]);
        assert_eq!(tile_grid_size([512, 513]), [1, 2]);
        assert_eq!(tile_grid_size([1, 1]), [1, 1]);
    }

    #[test]
    fn all_tiles_are_listed_row_by_row() {
        let coords: Vec<_> = all_tile_coords([1100, 600]).collect();
        assert_eq!(coords.len(), 6);
        assert_eq!(coords[0], TileCoord { x: 0, y: 0 });
        assert_eq!(coords[2], TileCoord { x: 2, y: 0 });
        assert_eq!(coords[3], TileCoord { x: 0, y: 1 });
    }

    #[test]
    fn rect_inside_one_tile_yields_one_span() {
        let spans: Vec<_> = tile_spans(rect(600, 10, 20, 30)).collect();
        assert_eq!(
            spans,
            vec![TileSpan {
                coord: TileCoord { x: 1, y: 0 },
                document_rect: rect(600, 10, 20, 30),
            }]
        );
        assert_eq!(spans[0].local_rect(), rect(88, 10, 20, 30));
    }

    #[test]
    fn rect_across_a_tile_corner_is_split_into_four_spans() {
        let spans: Vec<_> = tile_spans(rect(500, 490, 30, 40)).collect();
        assert_eq!(
            spans
                .iter()
                .map(|span| (span.coord, span.document_rect))
                .collect::<Vec<_>>(),
            vec![
                (TileCoord { x: 0, y: 0 }, rect(500, 490, 12, 22)),
                (TileCoord { x: 1, y: 0 }, rect(512, 490, 18, 22)),
                (TileCoord { x: 0, y: 1 }, rect(500, 512, 12, 18)),
                (TileCoord { x: 1, y: 1 }, rect(512, 512, 18, 18)),
            ]
        );
        assert_eq!(spans[3].local_rect(), rect(0, 0, 18, 18));
        let area: u32 = spans
            .iter()
            .map(|span| span.document_rect.width * span.document_rect.height)
            .sum();
        assert_eq!(area, 30 * 40);
    }

    #[test]
    fn rect_matching_a_tile_exactly_yields_that_tile() {
        let spans: Vec<_> = tile_spans(rect(512, 1024, 512, 512)).collect();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].coord, TileCoord { x: 1, y: 2 });
        assert_eq!(spans[0].local_rect(), rect(0, 0, 512, 512));
    }

    #[test]
    fn empty_rects_yield_no_spans() {
        assert_eq!(tile_spans(rect(10, 10, 0, 5)).count(), 0);
        assert_eq!(tile_spans(rect(10, 10, 5, 0)).count(), 0);
    }

    #[test]
    fn transparent_regions_are_detected() {
        let mut image = image::RgbaImage::new(600, 600);
        image.put_pixel(520, 3, image::Rgba([0, 0, 0, 1]));
        assert!(!region_has_alpha(&image, rect(0, 0, 512, 512)));
        assert!(region_has_alpha(&image, rect(512, 0, 88, 512)));
        assert!(!region_has_alpha(&image, rect(512, 4, 88, 512 - 4)));
    }

    #[test]
    fn shaders_share_the_tile_size() {
        for source in [
            include_str!("shaders/blit.wgsl"),
            include_str!("shaders/stroke_composite.wgsl"),
        ] {
            assert!(source.contains(&format!("const TILE_SIZE: i32 = {TILE_SIZE};")));
        }
        for source in [
            include_str!("shaders/layer_preview.wgsl"),
            include_str!("shaders/smudge.wgsl"),
        ] {
            assert!(source.contains(&format!("const TILE_SIZE: f32 = {TILE_SIZE}.0;")));
        }
    }

    #[test]
    fn edge_tiles_are_clipped_to_the_document() {
        assert_eq!(
            tile_document_rect(TileCoord { x: 2, y: 1 }, [1100, 700]),
            rect(1024, 512, 76, 188)
        );
        assert_eq!(
            tile_document_rect(TileCoord { x: 0, y: 0 }, [1100, 700]),
            rect(0, 0, 512, 512)
        );
    }
}
