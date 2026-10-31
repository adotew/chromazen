use super::*;
use crate::config::BrushCatalog;

#[test]
fn generated_preview_is_square_and_visible() {
    let brush = BrushCatalog::default()
        .brushes
        .into_iter()
        .next()
        .expect("bundled brush");

    let preview = generate(&brush).expect("preview");

    assert_eq!(preview.size, [SIZE as usize, SIZE as usize]);
    assert!(preview.pixels.iter().any(|pixel| pixel.a() > 0));
    assert!(
        preview
            .pixels
            .iter()
            .filter(|pixel| pixel.a() > 0)
            .all(|pixel| pixel.r() == pixel.g() && pixel.g() == pixel.b())
    );
}

#[test]
fn wide_stamp_preserves_aspect_ratio() {
    assert_eq!(fitted_size(192, 96), (104, 52));
    assert_eq!(fitted_size(96, 192), (52, 104));
}

#[test]
fn missing_stamp_returns_error() {
    let mut brush = BrushCatalog::default()
        .brushes
        .into_iter()
        .next()
        .expect("bundled brush");
    brush.preview.stamp_path = Some("missing-preview.png".into());

    assert!(generate(&brush).is_err());
}
