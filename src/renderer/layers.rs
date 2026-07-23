use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, Serialize)]
pub struct LayerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "id", rename_all = "camelCase")]
pub enum LayerSelection {
    Background,
    Paint(LayerId),
}

pub(crate) struct PaintLayer {
    pub(crate) id: LayerId,
    pub(crate) name: String,
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) blit_bind_group: wgpu::BindGroup,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    pub id: LayerId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSnapshot {
    pub layers: Vec<LayerInfo>,
    pub selection: LayerSelection,
    pub background_color: [f32; 4],
}

pub(crate) fn insertion_index(
    selection: LayerSelection,
    selected_index: Option<usize>,
    layer_count: usize,
) -> usize {
    match selection {
        LayerSelection::Background => 0,
        LayerSelection::Paint(_) => selected_index.map_or(layer_count, |index| index + 1),
    }
}

pub(crate) fn replacement_index_after_delete(
    layer_count: usize,
    deleted_index: usize,
) -> Option<usize> {
    (layer_count > 1).then(|| {
        if deleted_index > 0 {
            deleted_index - 1
        } else {
            1
        }
    })
}

pub(crate) fn layer_name(number: u64) -> String {
    format!("Layer {number}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_above_selection_or_background() {
        assert_eq!(insertion_index(LayerSelection::Background, None, 3), 0);
        assert_eq!(
            insertion_index(LayerSelection::Paint(LayerId(2)), Some(1), 3),
            2
        );
        assert_eq!(
            insertion_index(LayerSelection::Paint(LayerId(99)), None, 3),
            3
        );
    }

    #[test]
    fn deletion_prefers_layer_below() {
        assert_eq!(replacement_index_after_delete(3, 2), Some(1));
        assert_eq!(replacement_index_after_delete(3, 0), Some(1));
        assert_eq!(replacement_index_after_delete(1, 0), None);
    }

    #[test]
    fn names_are_monotonic() {
        assert_eq!(layer_name(1), "Layer 1");
        assert_eq!(layer_name(42), "Layer 42");
    }
}
