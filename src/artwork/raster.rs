use chromazen_canvas::{BlendMode, LayerSample, composite_samples};
use image::ImageEncoder;

pub(crate) struct CompositeLayer<'a> {
    pub(crate) image: &'a image::RgbaImage,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) clipped: bool,
    pub(crate) blend_mode: BlendMode,
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

    let background = [background[0], background[1], background[2], 255]
        .map(|channel| f32::from(channel) / 255.0);
    let mut samples = Vec::with_capacity(layers.len());
    let mut composite = image::RgbaImage::new(size.0, size.1);
    for (x, y, destination) in composite.enumerate_pixels_mut() {
        samples.clear();
        samples.extend(layers.iter().map(|layer| LayerSample {
            pixel: layer.image.get_pixel(x, y).0,
            opacity: layer.opacity,
            visible: layer.visible,
            clipped: layer.clipped,
            blend_mode: layer.blend_mode,
        }));
        let color = composite_samples(background, &samples);
        *destination =
            image::Rgba(color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8));
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
mod tests;
