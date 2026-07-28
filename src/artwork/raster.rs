use image::ImageEncoder;

use super::clipping_base_index;

pub(crate) struct CompositeLayer<'a> {
    pub(crate) image: &'a image::RgbaImage,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
}

/// Flattens bottom-to-top paint layers over an opaque background.
///
/// Layer RGB channels are expected to be premultiplied by their alpha, matching the
/// representation used by the GPU paint textures.
pub(crate) fn flatten_premultiplied_layers(
    layers: &[CompositeLayer<'_>],
    background: [u8; 3],
) -> Result<image::RgbaImage, String> {
    let Some(first_layer) = layers.first() else {
        return Err("cannot composite an artwork without layers".to_owned());
    };
    let size = first_layer.image.dimensions();
    if size.0 == 0 || size.1 == 0 {
        return Err("cannot composite an empty canvas".to_owned());
    }
    if layers.iter().any(|layer| layer.image.dimensions() != size) {
        return Err("composited layers must have matching dimensions".to_owned());
    }

    let mut composite = image::RgbaImage::from_pixel(
        size.0,
        size.1,
        image::Rgba([background[0], background[1], background[2], 255]),
    );
    let clipped: Vec<_> = layers.iter().map(|layer| layer.clipped).collect();
    for (index, layer) in layers.iter().enumerate().filter(|(_, layer)| layer.visible) {
        let base_index = clipping_base_index(&clipped, index);
        if layer.clipped && base_index.is_none()
            || base_index.is_some_and(|base_index| !layers[base_index].visible)
        {
            continue;
        }
        let base = base_index.map(|base_index| &layers[base_index]);
        let opacity = u32::from(layer.opacity.min(100));
        for (pixel_index, (destination, source)) in
            composite.pixels_mut().zip(layer.image.pixels()).enumerate()
        {
            let mask = base.map_or(255, |base| {
                u32::from(base.image.as_raw()[pixel_index * 4 + 3])
                    * u32::from(base.opacity.min(100))
                    / 100
            });
            let alpha = u32::from(source[3]) * opacity / 100 * mask / 255;
            let inverse = 255 - alpha;
            for channel in 0..3 {
                let source = u32::from(source[channel]) * opacity / 100 * mask / 255;
                destination[channel] =
                    (source + u32::from(destination[channel]) * inverse / 255).min(255) as u8;
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

    fn layer(image: &image::RgbaImage) -> CompositeLayer<'_> {
        CompositeLayer {
            image,
            visible: true,
            opacity: 100,
            clipped: false,
        }
    }

    #[test]
    fn transparent_pixels_reveal_the_background() {
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        let composite = flatten_premultiplied_layers(&[layer(&image)], [10, 20, 30]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn premultiplied_layers_composite_bottom_to_top() {
        let bottom = image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 0, 0, 128]));
        let top = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 128, 0, 128]));
        let composite =
            flatten_premultiplied_layers(&[layer(&bottom), layer(&top)], [0, 0, 255]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([63, 128, 63, 255]));
    }

    #[test]
    fn visibility_and_opacity_are_applied() {
        let red = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let green = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
        let layers = [
            CompositeLayer {
                image: &red,
                visible: true,
                opacity: 50,
                clipped: false,
            },
            CompositeLayer {
                image: &green,
                visible: false,
                opacity: 100,
                clipped: false,
            },
        ];
        let composite = flatten_premultiplied_layers(&layers, [0, 0, 255]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([127, 0, 128, 255]));
    }

    #[test]
    fn clipped_layer_uses_base_alpha_as_its_mask() {
        let base = image::RgbaImage::from_pixel(1, 1, image::Rgba([128, 0, 0, 128]));
        let clipped = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
        let layers = [
            CompositeLayer {
                image: &base,
                visible: true,
                opacity: 100,
                clipped: false,
            },
            CompositeLayer {
                image: &clipped,
                visible: true,
                opacity: 100,
                clipped: true,
            },
        ];
        let composite = flatten_premultiplied_layers(&layers, [0, 0, 255]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([63, 128, 63, 255]));
    }

    #[test]
    fn hidden_clipping_base_hides_clipped_layers() {
        let base = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let clipped = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]));
        let layers = [
            CompositeLayer {
                image: &base,
                visible: false,
                opacity: 100,
                clipped: false,
            },
            CompositeLayer {
                image: &clipped,
                visible: true,
                opacity: 100,
                clipped: true,
            },
        ];
        let composite = flatten_premultiplied_layers(&layers, [0, 0, 255]).unwrap();
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn native_dimensions_are_preserved_and_output_is_opaque() {
        let mut image = image::RgbaImage::new(7, 3);
        image.put_pixel(2, 1, image::Rgba([25, 50, 75, 100]));
        let composite = flatten_premultiplied_layers(&[layer(&image)], [255; 3]).unwrap();
        assert_eq!(composite.dimensions(), (7, 3));
        assert!(composite.pixels().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn invalid_layer_sets_are_rejected() {
        assert!(flatten_premultiplied_layers(&[], [255; 3]).is_err());
        let empty = image::RgbaImage::new(0, 1);
        assert!(flatten_premultiplied_layers(&[layer(&empty)], [255; 3]).is_err());
        let first = image::RgbaImage::new(1, 1);
        let second = image::RgbaImage::new(2, 1);
        assert!(flatten_premultiplied_layers(&[layer(&first), layer(&second)], [255; 3]).is_err());
    }

    #[test]
    fn png_round_trips() {
        let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([1, 2, 3, 255]));
        let encoded = encode_png(&image).unwrap();
        assert_eq!(image::load_from_memory(&encoded).unwrap().to_rgba8(), image);
    }
}
