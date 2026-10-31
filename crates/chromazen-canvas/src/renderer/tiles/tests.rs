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
        include_str!("../shaders/blit.wgsl"),
        include_str!("../shaders/stroke_composite.wgsl"),
    ] {
        assert!(source.contains(&format!("const TILE_SIZE: i32 = {TILE_SIZE};")));
    }
    for source in [
        include_str!("../shaders/layer_preview.wgsl"),
        include_str!("../shaders/smudge.wgsl"),
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
