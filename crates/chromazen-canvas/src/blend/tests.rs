use super::*;

const RED: [f32; 4] = [1.0, 0.2, 0.2, 1.0];

fn assert_close(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{actual:?} != {expected:?}"
        );
    }
}

fn sample(pixel: [u8; 4], blend_mode: BlendMode, clipped: bool) -> LayerSample {
    LayerSample {
        pixel,
        opacity: 100,
        visible: true,
        clipped,
        blend_mode,
    }
}

#[test]
fn normal_is_premultiplied_source_over() {
    let source = [0.0, 0.25, 0.0, 0.5];
    assert_close(
        blend_premultiplied(BlendMode::Normal, source, RED, false),
        [0.5, 0.35, 0.1, 1.0],
    );
}

#[test]
fn multiply_with_white_keeps_backdrop_and_black_gives_black() {
    let white = [1.0; 4];
    let black = [0.0, 0.0, 0.0, 1.0];
    assert_close(
        blend_premultiplied(BlendMode::Multiply, white, RED, false),
        RED,
    );
    assert_close(
        blend_premultiplied(BlendMode::Multiply, black, RED, false),
        black,
    );
    assert_close(
        blend_premultiplied(BlendMode::Multiply, [0.5, 0.5, 0.5, 1.0], RED, false),
        [0.5, 0.1, 0.1, 1.0],
    );
}

#[test]
fn translucent_multiply_mixes_with_backdrop() {
    assert_close(
        blend_premultiplied(BlendMode::Multiply, [0.0, 0.0, 0.0, 0.5], RED, false),
        [0.5, 0.1, 0.1, 1.0],
    );
}

#[test]
fn overlay_with_mid_gray_keeps_backdrop() {
    let backdrop = [0.25, 0.75, 0.5, 1.0];
    assert_close(
        blend_premultiplied(BlendMode::Overlay, [0.5, 0.5, 0.5, 1.0], backdrop, false),
        backdrop,
    );
}

#[test]
fn overlay_branches_on_backdrop_lightness() {
    assert_close(
        blend_premultiplied(BlendMode::Overlay, [1.0; 4], [0.25, 0.75, 0.0, 1.0], false),
        [0.5, 1.0, 0.0, 1.0],
    );
    assert_close(
        blend_premultiplied(
            BlendMode::Overlay,
            [0.0, 0.0, 0.0, 1.0],
            [0.25, 0.75, 1.0, 1.0],
            false,
        ),
        [0.0, 0.5, 1.0, 1.0],
    );
}

#[test]
fn blending_over_transparent_backdrop_returns_source() {
    let source = [0.1, 0.2, 0.3, 0.4];
    for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Overlay] {
        assert_close(blend_premultiplied(mode, source, [0.0; 4], false), source);
    }
}

#[test]
fn clipped_source_keeps_backdrop_alpha() {
    let backdrop = [0.5, 0.0, 0.0, 0.5];
    assert_close(
        blend_premultiplied(BlendMode::Normal, [0.0, 1.0, 0.0, 1.0], backdrop, true),
        [0.0, 0.5, 0.0, 0.5],
    );
    assert_close(
        blend_premultiplied(BlendMode::Multiply, [0.5, 0.5, 0.5, 1.0], backdrop, true),
        [0.25, 0.0, 0.0, 0.5],
    );
}

#[test]
fn clipped_multiply_shades_only_the_base() {
    let layers = [
        sample([255, 0, 0, 255], BlendMode::Normal, false),
        sample([128, 128, 128, 255], BlendMode::Multiply, true),
    ];
    let color = composite_samples([0.0, 0.0, 1.0, 1.0], &layers);
    assert_close(color, [128.0 / 255.0, 0.0, 0.0, 1.0]);
}

#[test]
fn base_mode_applies_to_its_clipping_group() {
    let layers = [
        sample([255, 255, 255, 255], BlendMode::Multiply, false),
        sample([128, 128, 128, 255], BlendMode::Normal, true),
    ];
    let color = composite_samples([1.0, 0.5, 0.0, 1.0], &layers);
    let gray = 128.0 / 255.0;
    assert_close(color, [gray, 0.5 * gray, 0.0, 1.0]);
}
