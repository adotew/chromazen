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
mod tests;
