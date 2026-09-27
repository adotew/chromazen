use super::history::TextureRect;

/// occupies 1 MiB, and a fully painted 4000x4000 layer uses an 8x8 grid.
pub(crate) const TILE_SIZE: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TileCoord {
    pub(crate) x: u32,
    pub(crate) y: u32,
}

impl TileCoord {
    pub(crate) fn origin(self) -> [u32; 2] {
        [self.x * TILE_SIZE, self.y * TILE_SIZE]
    }

    /// The document rectangle covered by this tile, including pixels beyond the document edge.
    pub(crate) fn rect(self) -> TextureRect {
        let [x, y] = self.origin();
        TextureRect {
            x,
            y,
            width: TILE_SIZE,
            height: TILE_SIZE,
        }
    }
}

/// The part of a document rectangle that falls inside one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TileSpan {
    pub(crate) coord: TileCoord,
    pub(crate) document: TextureRect,
}

impl TileSpan {
    pub(crate) fn local(self) -> TextureRect {
        let [x, y] = self.coord.origin();
        TextureRect {
            x: self.document.x - x,
            y: self.document.y - y,
            ..self.document
        }
    }
}

pub(crate) fn grid_size(document_size: [u32; 2]) -> [u32; 2] {
    document_size.map(|dimension| dimension.div_ceil(TILE_SIZE))
}

pub(crate) fn all_tiles(document_size: [u32; 2]) -> impl Iterator<Item = TileCoord> {
    let [columns, rows] = grid_size(document_size);
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
                document: TextureRect {
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
        assert_eq!(grid_size([4000, 4000]), [8, 8]);
        assert_eq!(grid_size([512, 513]), [1, 2]);
        assert_eq!(grid_size([1, 1]), [1, 1]);
    }

    #[test]
    fn all_tiles_are_listed_row_by_row() {
        let coords: Vec<_> = all_tiles([1100, 600]).collect();
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
                document: rect(600, 10, 20, 30),
            }]
        );
        assert_eq!(spans[0].local(), rect(88, 10, 20, 30));
    }

    #[test]
    fn rect_across_a_tile_corner_is_split_into_four_spans() {
        let spans: Vec<_> = tile_spans(rect(500, 490, 30, 40)).collect();
        assert_eq!(
            spans
                .iter()
                .map(|span| (span.coord, span.document))
                .collect::<Vec<_>>(),
            vec![
                (TileCoord { x: 0, y: 0 }, rect(500, 490, 12, 22)),
                (TileCoord { x: 1, y: 0 }, rect(512, 490, 18, 22)),
                (TileCoord { x: 0, y: 1 }, rect(500, 512, 12, 18)),
                (TileCoord { x: 1, y: 1 }, rect(512, 512, 18, 18)),
            ]
        );
        assert_eq!(spans[3].local(), rect(0, 0, 18, 18));
        let area: u32 = spans
            .iter()
            .map(|span| span.document.width * span.document.height)
            .sum();
        assert_eq!(area, 30 * 40);
    }

    #[test]
    fn rect_matching_a_tile_exactly_yields_that_tile() {
        let spans: Vec<_> = tile_spans(rect(512, 1024, 512, 512)).collect();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].coord, TileCoord { x: 1, y: 2 });
        assert_eq!(spans[0].local(), rect(0, 0, 512, 512));
    }

    #[test]
    fn empty_rects_yield_no_spans() {
        assert_eq!(tile_spans(rect(10, 10, 0, 5)).count(), 0);
        assert_eq!(tile_spans(rect(10, 10, 5, 0)).count(), 0);
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
        assert_eq!(TileCoord { x: 2, y: 1 }.rect(), rect(1024, 512, 512, 512));
    }
}
