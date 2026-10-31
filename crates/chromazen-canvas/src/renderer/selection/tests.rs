use super::*;

#[test]
fn square_is_filled_inside_and_empty_outside() {
    let mask = rasterize(
        &[[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]],
        [64, 64],
    )
    .expect("square covers pixels");
    assert_eq!(
        mask.rect,
        TextureRect {
            x: 10,
            y: 10,
            width: 10,
            height: 10
        }
    );
    assert_eq!(mask.at(10, 10), 255);
    assert_eq!(mask.at(19, 19), 255);
    assert_eq!(mask.at(9, 15), 0);
    assert_eq!(mask.at(20, 15), 0);
}

#[test]
fn edges_crossing_pixels_are_antialiased() {
    let mask = rasterize(
        &[[0.0, 0.0], [10.5, 0.0], [10.5, 10.0], [0.0, 10.0]],
        [64, 64],
    )
    .expect("rectangle covers pixels");
    assert_eq!(mask.at(9, 5), 255);
    assert!((127..=128).contains(&mask.at(10, 5)));
}

#[test]
fn overlapping_loops_use_the_nonzero_rule() {
    let square = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]];
    let twice: Vec<_> = square.iter().chain(&square).copied().collect();
    let mask = rasterize(&twice, [16, 16]).expect("square covers pixels");
    assert_eq!(mask.at(4, 4), 255);
}

#[test]
fn coverage_is_clipped_to_the_document() {
    let mask = rasterize(
        &[[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]],
        [4, 4],
    )
    .expect("polygon overlaps the document");
    assert_eq!(
        mask.rect,
        TextureRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4
        }
    );
    assert!(mask.coverage.iter().all(|&value| value == 255));
}

#[test]
fn degenerate_or_outside_polygons_select_nothing() {
    assert!(rasterize(&[[0.0, 0.0], [5.0, 5.0]], [16, 16]).is_none());
    assert!(rasterize(&[[0.0, 0.0], [5.0, 5.0], [10.0, 10.0]], [16, 16]).is_none());
    assert!(rasterize(&[[20.0, 20.0], [30.0, 20.0], [30.0, 30.0]], [16, 16]).is_none());
    assert!(rasterize(&[[0.0, 0.0], [f32::NAN, 5.0], [10.0, 10.0]], [16, 16]).is_none());
}
