#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal = 0,
    Multiply = 1,
    Overlay = 2,
}

impl BlendMode {
    /// Separable blend function `B(Cb, Cs)` on unpremultiplied channels.
    fn channel(self, backdrop: f32, source: f32) -> f32 {
        match self {
            Self::Normal => source,
            Self::Multiply => backdrop * source,
            Self::Overlay if backdrop <= 0.5 => 2.0 * backdrop * source,
            Self::Overlay => 1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source),
        }
    }
}

/// Composites premultiplied `source` over premultiplied `backdrop` (W3C Compositing Level 1):
/// `co = cs·(1−αb) + cb·(1−αs) + αs·αb·B(Cb, Cs)`.
///
/// A clipped source only shows where the backdrop has coverage, so it drops the
/// `cs·(1−αb)` term and keeps the backdrop's alpha.
pub fn blend_premultiplied(
    mode: BlendMode,
    source: [f32; 4],
    backdrop: [f32; 4],
    clipped: bool,
) -> [f32; 4] {
    let (source_alpha, backdrop_alpha) = (source[3], backdrop[3]);
    let mut output = [0.0; 4];
    for channel in 0..3 {
        let (cs, cb) = (source[channel], backdrop[channel]);
        let blended = if source_alpha > 0.0 && backdrop_alpha > 0.0 {
            let unpremultiplied =
                mode.channel((cb / backdrop_alpha).min(1.0), (cs / source_alpha).min(1.0));
            source_alpha * backdrop_alpha * unpremultiplied
        } else {
            0.0
        };
        let uncovered_source = if clipped {
            0.0
        } else {
            cs * (1.0 - backdrop_alpha)
        };
        output[channel] = uncovered_source + cb * (1.0 - source_alpha) + blended;
    }
    output[3] = if clipped {
        backdrop_alpha
    } else {
        source_alpha + backdrop_alpha * (1.0 - source_alpha)
    };
    output
}

/// One layer's premultiplied texel and the properties that affect compositing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerSample {
    pub pixel: [u8; 4],
    pub opacity: u8,
    pub visible: bool,
    pub clipped: bool,
    pub blend_mode: BlendMode,
}

/// Composites bottom-to-top layer samples over a premultiplied background.
///
/// Clipped layers blend into their base's isolated group, and the group then blends over the
/// layers below with the base's mode.
pub fn composite_samples(background: [f32; 4], layers: &[LayerSample]) -> [f32; 4] {
    let mut color = background;
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
        let base = layers[base_index];
        if base.visible {
            let mut group = premultiplied_with_opacity(base);
            for layer in layers[base_index + 1..group_end]
                .iter()
                .filter(|layer| layer.visible)
            {
                group = blend_premultiplied(
                    layer.blend_mode,
                    premultiplied_with_opacity(*layer),
                    group,
                    true,
                );
            }
            color = blend_premultiplied(base.blend_mode, group, color, false);
        }
        base_index = group_end;
    }
    color
}

fn premultiplied_with_opacity(layer: LayerSample) -> [f32; 4] {
    let opacity = f32::from(layer.opacity.min(100)) / 100.0;
    layer
        .pixel
        .map(|channel| f32::from(channel) / 255.0 * opacity)
}

#[cfg(test)]
mod tests {
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
        // Half-covered black: cb·(1−αs) + cs·cb = 0.5·cb.
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
        // Opaque red base with a clipped gray Multiply layer, over a blue background.
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
}
