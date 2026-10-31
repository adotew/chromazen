use chromazen_canvas::BlendMode;
use serde::{Deserialize, Serialize};

pub(crate) const PROJECT_SCHEMA_VERSION: u32 = 1;
pub(crate) const DOCUMENT_SCHEMA_VERSION: u32 = 5;
const ORIENTATIONLESS_DOCUMENT_SCHEMA_VERSION: u32 = 4;
const BLEND_MODELESS_DOCUMENT_SCHEMA_VERSION: u32 = 3;
const LEGACY_DOCUMENT_SCHEMA_VERSION: u32 = 2;

fn default_brush_color() -> [u8; 4] {
    [170, 187, 204, 255]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectManifest {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) current_revision: u64,
    pub(crate) modified_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentManifest {
    pub(crate) schema_version: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) background: [u8; 3],
    #[serde(default = "default_brush_color")]
    pub(crate) brush_color: [u8; 4],
    /// Canvas view rotation in radians; flips are horizontal then vertical and apply first.
    #[serde(default)]
    pub(crate) rotation: f32,
    #[serde(default)]
    pub(crate) flip: [bool; 2],
    pub(crate) selected_layer: u64,
    pub(crate) layers: Vec<LayerManifest>,
    #[serde(default)]
    pub(crate) references: Vec<ReferenceManifest>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReferenceManifest {
    pub(crate) id: u64,
    pub(crate) file: String,
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) visible: bool,
    pub(crate) locked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LayerManifest {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    #[serde(default)]
    pub(crate) clipped: bool,
    #[serde(default)]
    pub(crate) blend_mode: BlendMode,
    pub(crate) file: String,
}

impl DocumentManifest {
    pub(crate) fn migrate(mut self) -> Result<Self, String> {
        match self.schema_version {
            DOCUMENT_SCHEMA_VERSION => Ok(self),
            ORIENTATIONLESS_DOCUMENT_SCHEMA_VERSION | BLEND_MODELESS_DOCUMENT_SCHEMA_VERSION => {
                self.schema_version = DOCUMENT_SCHEMA_VERSION;
                Ok(self)
            }
            LEGACY_DOCUMENT_SCHEMA_VERSION => {
                self.schema_version = DOCUMENT_SCHEMA_VERSION;
                self.references.clear();
                Ok(self)
            }
            version => Err(format!(
                "unsupported document schema_version {version}; expected {DOCUMENT_SCHEMA_VERSION}"
            )),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != DOCUMENT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported document schema_version {}; expected {DOCUMENT_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if self.width == 0 || self.height == 0 {
            return Err("document dimensions must be non-zero".to_owned());
        }
        if !self.rotation.is_finite() {
            return Err("document rotation must be finite".to_owned());
        }
        if self.layers.is_empty() {
            return Err("document must contain at least one layer".to_owned());
        }
        if !self
            .layers
            .iter()
            .any(|layer| layer.id == self.selected_layer)
        {
            return Err("selected_layer does not identify a document layer".to_owned());
        }
        if self.layers[0].clipped {
            return Err("bottom layer cannot be clipped".to_owned());
        }
        let mut ids = std::collections::HashSet::new();
        let mut files = std::collections::HashSet::new();
        for layer in &self.layers {
            if layer.id == 0 || !ids.insert(layer.id) {
                return Err("layer IDs must be non-zero and unique".to_owned());
            }
            if layer.name.trim().is_empty() {
                return Err("layer names must not be empty".to_owned());
            }
            if layer.opacity > 100 {
                return Err("layer opacity must be between 0 and 100".to_owned());
            }
            if layer.file != format!("layers/{}.png", layer.id) || !files.insert(&layer.file) {
                return Err(format!("invalid layer file '{}'", layer.file));
            }
        }

        let mut reference_ids = std::collections::HashSet::new();
        let mut reference_files = std::collections::HashSet::new();
        for reference in &self.references {
            if reference.id == 0 || !reference_ids.insert(reference.id) {
                return Err("reference IDs must be non-zero and unique".to_owned());
            }
            if reference.file != format!("references/{}.png", reference.id)
                || !reference_files.insert(&reference.file)
            {
                return Err(format!("invalid reference file '{}'", reference.file));
            }
            if reference
                .position
                .into_iter()
                .chain(reference.size)
                .any(|value| !value.is_finite())
            {
                return Err("reference position and size must be finite".to_owned());
            }
            if reference.size.into_iter().any(|value| value <= 0.0) {
                return Err("reference width and height must be positive".to_owned());
            }
        }
        Ok(())
    }
}

pub(crate) fn normalized_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        Err("artwork title must not be empty".to_owned())
    } else {
        Ok(title.to_owned())
    }
}

#[cfg(test)]
mod tests;
