/// Discriminants are shared with `blend.wgsl`; serialized names are persisted in artworks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlendMode {
    #[default]
    Normal = 0,
    Multiply = 1,
    Overlay = 2,
}

impl BlendMode {
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
/// Clipping drops `cs·(1−αb)` and preserves the backdrop's alpha.
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

/// A premultiplied RGBA8 layer sample.
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
/// Clips blend within their base's isolated group; the base's mode then blends the whole group.
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
mod tests;
