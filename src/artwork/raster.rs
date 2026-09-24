use image::ImageEncoder;

pub(crate) struct CompositeLayer<'a> {
    pub(crate) image: &'a image::RgbaImage,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
}

/// Validated premultiplied compositor. The same row operation serves the
/// legacy image adapter and streaming export; future tile readers can supply
/// source bands without changing blend order or rounding.
pub(crate) struct CompositeRows<'a> {
    layers: &'a [CompositeLayer<'a>],
    background: [u8; 3],
    size: [u32; 2],
}

impl<'a> CompositeRows<'a> {
    pub(crate) fn new(
        layers: &'a [CompositeLayer<'a>],
        background: [u8; 3],
    ) -> Result<Self, String> {
        let Some(first) = layers.first() else {
            return Err("cannot composite an artwork without layers".to_owned());
        };
        let (width, height) = first.image.dimensions();
        if width == 0 || height == 0 {
            return Err("cannot composite an empty canvas".to_owned());
        }
        if layers
            .iter()
            .any(|layer| layer.image.dimensions() != (width, height))
        {
            return Err("composited layers must have matching dimensions".to_owned());
        }
        Ok(Self {
            layers,
            background,
            size: [width, height],
        })
    }

    pub(crate) fn size(&self) -> [u32; 2] {
        self.size
    }

    pub(crate) fn row(&self, y: u32, output: &mut [u8]) {
        assert!(y < self.size[1]);
        assert_eq!(output.len(), self.size[0] as usize * 4);
        let row_start = y as usize * output.len();
        let sources: Vec<_> = self
            .layers
            .iter()
            .map(|layer| CompositeRowLayer {
                pixels: &layer.image.as_raw()[row_start..row_start + output.len()],
                visible: layer.visible,
                opacity: layer.opacity,
                clipped: layer.clipped,
            })
            .collect();
        composite_premultiplied_row(&sources, self.background, output);
    }
}

/// Source rows are borrowed: no allocation per decoded layer is needed during
/// export. Every source must have the same RGBA8 width as `output`.
pub(crate) struct CompositeRowLayer<'a> {
    pub(crate) pixels: &'a [u8],
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
}

pub(crate) fn composite_premultiplied_row(
    layers: &[CompositeRowLayer<'_>],
    background: [u8; 3],
    output: &mut [u8],
) {
    assert!(
        layers
            .iter()
            .all(|layer| layer.pixels.len() == output.len())
    );
    assert!(output.len().is_multiple_of(4));
    for (x, destination) in output.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        destination.copy_from_slice(&[background[0], background[1], background[2], 255]);
        let pixel_index = x * 4;
        let mut base_index = 0;
        while base_index < layers.len() {
            if layers[base_index].clipped {
                base_index += 1;
                continue;
            }
            let mut group_end = base_index + 1;
            while group_end < layers.len() && layers[group_end].clipped {
                group_end += 1;
            }
            let base = &layers[base_index];
            if base.visible {
                let base_pixel = &base.pixels[pixel_index..pixel_index + 4];
                let base_opacity = u32::from(base.opacity.min(100));
                let base_alpha = u32::from(base_pixel[3]) * base_opacity / 100;
                let mut group_rgb = [0; 3];
                for channel in 0..3 {
                    group_rgb[channel] = u32::from(base_pixel[channel]) * base_opacity / 100;
                }
                for layer in layers[base_index + 1..group_end]
                    .iter()
                    .filter(|layer| layer.visible)
                {
                    let source = &layer.pixels[pixel_index..pixel_index + 4];
                    let opacity = u32::from(layer.opacity.min(100));
                    let alpha = u32::from(source[3]) * opacity / 100;
                    let inverse = 255 - alpha;
                    for channel in 0..3 {
                        let source = u32::from(source[channel]) * opacity / 100 * base_alpha / 255;
                        group_rgb[channel] = (source + group_rgb[channel] * inverse / 255).min(255);
                    }
                }
                let inverse_base = 255 - base_alpha;
                for channel in 0..3 {
                    destination[channel] = (group_rgb[channel]
                        + u32::from(destination[channel]) * inverse_base / 255)
                        .min(255) as u8;
                }
            }
            base_index = group_end;
        }
    }
}

/// Flattens bottom-to-top paint layers over an opaque background.
/// Layer RGB channels are premultiplied by alpha, matching GPU textures.
pub(crate) fn flatten_premultiplied_layers(
    layers: &[CompositeLayer<'_>],
    background: [u8; 3],
) -> Result<image::RgbaImage, String> {
    let rows = CompositeRows::new(layers, background)?;
    let [width, height] = rows.size();
    let mut image = image::RgbaImage::new(width, height);
    for (y, output) in image
        .as_mut()
        .chunks_exact_mut(width as usize * 4)
        .enumerate()
    {
        rows.row(y as u32, output);
    }
    Ok(image)
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
    fn clipped_layer_recolors_translucent_base_without_revealing_its_color() {
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
        assert_eq!(composite.get_pixel(0, 0), &image::Rgba([0, 128, 127, 255]));
    }

    #[test]
    fn opaque_black_clip_does_not_leave_translucent_base_tinted() {
        let base = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 64, 128, 128]));
        let black = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        let layers = [
            CompositeLayer {
                image: &base,
                visible: true,
                opacity: 100,
                clipped: false,
            },
            CompositeLayer {
                image: &black,
                visible: true,
                opacity: 100,
                clipped: true,
            },
        ];
        let composite = flatten_premultiplied_layers(&layers, [255; 3]).unwrap();
        assert_eq!(
            composite.get_pixel(0, 0),
            &image::Rgba([127, 127, 127, 255])
        );
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
