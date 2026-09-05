use image::{Rgba, RgbaImage, imageops::FilterType};

use crate::config::BrushSummary;

const SIZE: u32 = 128;
const PADDING: u32 = 12;

pub(super) fn generate(brush: &BrushSummary) -> Result<egui::ColorImage, String> {
    let stamp = brush
        .load_preview_stamp()
        .map_err(|error| error.to_string())?;
    let (width, height) = fitted_size(stamp.width(), stamp.height());
    let stamp = image::imageops::resize(&stamp, width, height, FilterType::Lanczos3);
    let mut preview = RgbaImage::new(SIZE, SIZE);
    let left = (SIZE - width) / 2;
    let top = (SIZE - height) / 2;

    for (x, y, source) in stamp.enumerate_pixels() {
        preview.put_pixel(left + x, top + y, Rgba([255, 255, 255, source[3]]));
    }

    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [SIZE as usize, SIZE as usize],
        preview.as_raw(),
    ))
}

fn fitted_size(width: u32, height: u32) -> (u32, u32) {
    let available = SIZE - 2 * PADDING;
    let scale = available as f32 / width.max(height).max(1) as f32;
    (
        (width as f32 * scale).round().max(1.0) as u32,
        (height as f32 * scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
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
}
