use image::{imageops::FilterType, Rgba, RgbaImage};

use crate::config::BrushSummary;

const WIDTH: u32 = 192;
const HEIGHT: u32 = 48;
const MAX_RADIUS: f32 = 15.0;

pub(super) fn generate(brush: &BrushSummary) -> Result<egui::ColorImage, String> {
    let stamp = brush
        .load_preview_stamp()
        .map_err(|error| error.to_string())?;
    let aspect = stamp.width() as f32 / stamp.height() as f32;
    let (max_half_width, max_half_height) = stamp_half_size(MAX_RADIUS, aspect);
    let stamp = image::imageops::resize(
        &stamp,
        (max_half_width * 2.0).round().max(1.0) as u32,
        (max_half_height * 2.0).round().max(1.0) as u32,
        FilterType::Triangle,
    );
    let mut preview = RgbaImage::new(WIDTH, HEIGHT);
    let mut x = MAX_RADIUS;
    let end_x = WIDTH as f32 - MAX_RADIUS;
    let preview_scale = MAX_RADIUS * 2.0 / brush.preview.size.default;

    while x <= end_x {
        let t = (x - MAX_RADIUS) / (end_x - MAX_RADIUS);
        let pressure = 0.18 + 0.82 * (std::f32::consts::PI * t).sin().max(0.0).sqrt();
        let pressure_scale =
            brush.preview.pressure.min_size + (1.0 - brush.preview.pressure.min_size) * pressure;
        let radius = MAX_RADIUS * pressure_scale;
        let opacity_pressure = (pressure / brush.preview.pressure.full_opacity_pressure).min(1.0);
        let opacity = brush.preview.pressure.min_opacity
            + (1.0 - brush.preview.pressure.min_opacity)
                * opacity_pressure.powf(brush.preview.pressure.opacity_gamma);
        let y = HEIGHT as f32 * 0.5 + (t * std::f32::consts::TAU).sin() * 3.0;
        paint_stamp(&mut preview, &stamp, aspect, x, y, radius, opacity);
        x += (radius * brush.preview.spacing.ratio)
            .max(brush.preview.spacing.minimum * preview_scale)
            .max(1.0);
    }

    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [WIDTH as usize, HEIGHT as usize],
        preview.as_raw(),
    ))
}

fn paint_stamp(
    preview: &mut RgbaImage,
    stamp: &RgbaImage,
    aspect: f32,
    center_x: f32,
    center_y: f32,
    radius: f32,
    opacity: f32,
) {
    let (half_width, half_height) = stamp_half_size(radius, aspect);
    let stamp = image::imageops::resize(
        stamp,
        (half_width * 2.0).round().max(1.0) as u32,
        (half_height * 2.0).round().max(1.0) as u32,
        FilterType::Triangle,
    );
    let left = (center_x - stamp.width() as f32 * 0.5).round() as i32;
    let top = (center_y - stamp.height() as f32 * 0.5).round() as i32;

    for (stamp_x, stamp_y, source) in stamp.enumerate_pixels() {
        let x = left + stamp_x as i32;
        let y = top + stamp_y as i32;
        if x < 0 || y < 0 || x >= preview.width() as i32 || y >= preview.height() as i32 {
            continue;
        }
        let source_alpha = source[3] as f32 / 255.0 * opacity;
        let target = preview.get_pixel_mut(x as u32, y as u32);
        let target_alpha = target[3] as f32 / 255.0;
        let alpha = source_alpha.max(target_alpha);
        *target = Rgba([255, 255, 255, (alpha * 255.0).round() as u8]);
    }
}

fn stamp_half_size(radius: f32, aspect: f32) -> (f32, f32) {
    if aspect >= 1.0 {
        (radius, radius / aspect)
    } else {
        (radius * aspect, radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BrushCatalog;

    #[test]
    fn generated_preview_has_visible_pixels() {
        let brush = BrushCatalog::default()
            .brushes
            .into_iter()
            .next()
            .expect("bundled brush");

        let preview = generate(&brush).expect("preview");

        assert_eq!(preview.size, [WIDTH as usize, HEIGHT as usize]);
        assert!(preview.pixels.iter().any(|pixel| pixel.a() > 0));
    }

    #[test]
    fn repeated_stamps_use_maximum_coverage() {
        let mut preview = RgbaImage::new(1, 1);
        let stamp = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]));

        paint_stamp(&mut preview, &stamp, 1.0, 0.5, 0.5, 0.5, 0.25);
        let first_alpha = preview.get_pixel(0, 0)[3];
        paint_stamp(&mut preview, &stamp, 1.0, 0.5, 0.5, 0.5, 0.25);

        assert_eq!(first_alpha, 64);
        assert_eq!(preview.get_pixel(0, 0)[3], first_alpha);
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
