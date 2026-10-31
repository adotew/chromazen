use super::*;
use crate::BlendMode;

fn sample(pixel: [u8; 4], opacity: u8, visible: bool, clipped: bool) -> LayerSample {
    LayerSample {
        pixel,
        opacity,
        visible,
        clipped,
        blend_mode: BlendMode::Normal,
    }
}

fn composite(background: [f32; 4], layers: &[LayerSample]) -> [u8; 3] {
    rgb8(composite_samples(background, layers))
}

#[test]
fn maps_document_points_to_pixels() {
    assert_eq!(document_pixel([12.9, 4.1], [100, 50]), Some([12, 4]));
    assert_eq!(document_pixel([0.0, 0.0], [100, 50]), Some([0, 0]));
    assert_eq!(document_pixel([99.99, 49.99], [100, 50]), Some([99, 49]));
}

#[test]
fn rejects_points_outside_the_document() {
    for point in [
        [-0.1, 2.0],
        [2.0, -0.1],
        [100.0, 2.0],
        [2.0, 50.0],
        [f32::NAN, 2.0],
    ] {
        assert_eq!(document_pixel(point, [100, 50]), None);
    }
}

#[test]
fn transparent_layers_reveal_the_background() {
    assert_eq!(
        composite(
            [0.2, 0.4, 0.8, 1.0],
            &[sample([0, 0, 0, 0], 100, true, false)]
        ),
        [51, 102, 204]
    );
}

#[test]
fn composites_premultiplied_layers_bottom_to_top() {
    // Half-red over blue, followed by half-green over that result.
    let pixels = [
        sample([128, 0, 0, 128], 100, true, false),
        sample([0, 128, 0, 128], 100, true, false),
    ];
    assert_eq!(composite([0.0, 0.0, 1.0, 1.0], &pixels), [64, 128, 63]);
}

#[test]
fn layer_opacity_scales_premultiplied_color_and_alpha() {
    assert_eq!(
        composite(
            [0.0, 0.0, 1.0, 1.0],
            &[sample([255, 0, 0, 255], 50, true, false)]
        ),
        [128, 0, 128]
    );
}

#[test]
fn clipped_samples_recolor_translucent_base_without_revealing_its_color() {
    let pixels = [
        sample([128, 0, 0, 128], 100, true, false),
        sample([0, 255, 0, 255], 100, true, true),
    ];
    assert_eq!(composite([0.0, 0.0, 1.0, 1.0], &pixels), [0, 128, 127]);
}

#[test]
fn hidden_base_hides_clipped_samples() {
    let pixels = [
        sample([255, 0, 0, 255], 100, false, false),
        sample([0, 255, 0, 255], 100, true, true),
    ];
    assert_eq!(composite([0.0, 0.0, 1.0, 1.0], &pixels), [0, 0, 255]);
}

#[test]
fn opaque_top_layer_wins() {
    let pixels = [
        sample([255, 0, 0, 255], 100, true, false),
        sample([12, 34, 56, 255], 100, true, false),
    ];
    assert_eq!(composite([1.0, 1.0, 1.0, 1.0], &pixels), [12, 34, 56]);
}
