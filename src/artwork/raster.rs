use image::ImageEncoder;

/// Flattens bottom-to-top paint layers over an opaque background.
///
/// Layer RGB channels are expected to be premultiplied by their alpha, matching the
/// representation used by the GPU paint textures.
pub(crate) fn flatten_premultiplied_layers(
    layers: &[image::RgbaImage],
    background: [u8; 3],
) -> Result<image::RgbaImage, String> {
    let Some(first_layer) = layers.first() else {
        return Err("cannot composite an artwork without layers".to_owned());
    };
    let size = first_layer.dimensions();
    if size.0 == 0 || size.1 == 0 {
        return Err("cannot composite an empty canvas".to_owned());
    }
    if layers.iter().any(|layer| layer.dimensions() != size) {
        return Err("composited layers must have matching dimensions".to_owned());
    }

    let mut composite = image::RgbaImage::from_pixel(
        size.0,
        size.1,
        image::Rgba([background[0], background[1], background[2], 255]),
    );
    for layer in layers {
        for (destination, source) in composite.pixels_mut().zip(layer.pixels()) {
            let alpha = u32::from(source[3]);
            let inverse = 255 - alpha;
            for channel in 0..3 {
                destination[channel] = (u32::from(source[channel])
                    + u32::from(destination[channel]) * inverse / 255)
                    .min(255) as u8;
            }
        }
    }
    Ok(composite)
}

pub(crate) fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("failed to encode PNG: {error}"))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_pixels_reveal_the_background() {
        let layer = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        let composite = flatten_premultiplied_layers(&[layer], [10, 20, 30]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn premultiplied_layers_composite_bottom_to_top() {
        let bottom = image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 0, 0, 128]));
        let top = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 128, 0, 128]));
        let composite = flatten_premultiplied_layers(&[bottom, top], [0, 0, 255]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([63, 128, 63, 255]));
    }

    #[test]
    fn native_dimensions_are_preserved() {
        let layer = image::RgbaImage::new(7, 3);
        let composite = flatten_premultiplied_layers(&[layer], [255; 3]).unwrap();
        assert_eq!(composite.dimensions(), (7, 3));
    }

    #[test]
    fn invalid_layer_sets_are_rejected() {
        assert!(flatten_premultiplied_layers(&[], [255; 3]).is_err());
        let layers = [image::RgbaImage::new(1, 1), image::RgbaImage::new(2, 1)];
        assert!(flatten_premultiplied_layers(&layers, [255; 3]).is_err());
    }

    #[test]
    fn png_round_trips() {
        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]));
        let encoded = encode_png(&image).unwrap();
        assert_eq!(image::load_from_memory(&encoded).unwrap().to_rgba8(), image);
    }
}
