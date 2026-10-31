mod format;
mod raster;
mod store;

use chromazen_canvas::{CanvasDocument, LayerId, LayerInfo};

pub(crate) use format::{
    DOCUMENT_SCHEMA_VERSION, DocumentManifest, LayerManifest, ReferenceManifest,
};
pub(crate) use raster::{CompositeLayer, encode_png, flatten_premultiplied_layers};
pub(crate) use store::{
    ArtworkId, ArtworkStore, ArtworkSummary, LayerSource, LayerWrite, ReferenceSource,
    ReferenceWrite, RevisionWrite,
};

pub(crate) fn canvas_document(document: &DocumentManifest) -> CanvasDocument {
    CanvasDocument {
        size: [document.width, document.height],
        background: document.background,
        selected_layer: LayerId(document.selected_layer),
        layers: document
            .layers
            .iter()
            .map(|layer| LayerInfo {
                id: LayerId(layer.id),
                name: layer.name.clone(),
                visible: layer.visible,
                opacity: layer.opacity,
                clipped: layer.clipped,
                blend_mode: layer.blend_mode,
            })
            .collect(),
    }
}

pub(crate) fn document_manifest(document: CanvasDocument) -> DocumentManifest {
    DocumentManifest {
        schema_version: DOCUMENT_SCHEMA_VERSION,
        width: document.size[0],
        height: document.size[1],
        background: document.background,
        brush_color: [170, 187, 204, 255],
        rotation: 0.0,
        flip: [false; 2],
        selected_layer: document.selected_layer.0,
        layers: document
            .layers
            .into_iter()
            .map(|layer| LayerManifest {
                id: layer.id.0,
                name: layer.name,
                visible: layer.visible,
                opacity: layer.opacity,
                clipped: layer.clipped,
                blend_mode: layer.blend_mode,
                file: format!("layers/{}.png", layer.id.0),
            })
            .collect(),
        references: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
