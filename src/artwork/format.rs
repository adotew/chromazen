use serde::{Deserialize, Serialize};

pub(crate) const PROJECT_SCHEMA_VERSION: u32 = 1;
pub(crate) const DOCUMENT_SCHEMA_VERSION: u32 = 3;
const LEGACY_DOCUMENT_SCHEMA_VERSION: u32 = 2;

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
    pub(crate) file: String,
}

impl DocumentManifest {
    pub(crate) fn migrate(mut self) -> Result<Self, String> {
        match self.schema_version {
            DOCUMENT_SCHEMA_VERSION => Ok(self),
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
mod tests {
    use super::*;

    fn document() -> DocumentManifest {
        DocumentManifest {
            schema_version: DOCUMENT_SCHEMA_VERSION,
            width: 4000,
            height: 4000,
            background: [255; 3],
            selected_layer: 1,
            layers: vec![LayerManifest {
                id: 1,
                name: "Layer 1".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
                file: "layers/1.png".to_owned(),
            }],
            references: Vec::new(),
        }
    }

    #[test]
    fn valid_document_is_accepted() {
        assert!(document().validate().is_ok());
    }

    #[test]
    fn selected_layer_must_exist() {
        let mut document = document();
        document.selected_layer = 2;
        assert!(document.validate().is_err());
    }

    #[test]
    fn bottom_layer_cannot_be_clipped() {
        let mut document = document();
        document.layers[0].clipped = true;
        assert!(document.validate().is_err());
    }

    #[test]
    fn invalid_opacity_is_rejected() {
        let mut document = document();
        document.layers[0].opacity = 101;
        assert!(document.validate().is_err());
    }

    #[test]
    fn clipping_defaults_to_disabled_for_existing_documents() {
        let source = toml::to_string(&document()).unwrap();
        let without_clipping = source.replace("clipped = false\n", "");
        let decoded: DocumentManifest = toml::from_str(&without_clipping).unwrap();
        assert!(!decoded.layers[0].clipped);
    }

    #[test]
    fn layer_metadata_is_required() {
        let source = toml::to_string(&document()).unwrap();
        let without_visibility = source.replace("visible = true\n", "");
        let without_opacity = source.replace("opacity = 100\n", "");
        assert!(toml::from_str::<DocumentManifest>(&without_visibility).is_err());
        assert!(toml::from_str::<DocumentManifest>(&without_opacity).is_err());
    }

    #[test]
    fn version_two_document_is_migrated_without_references() {
        let mut document = document();
        document.schema_version = LEGACY_DOCUMENT_SCHEMA_VERSION;
        let migrated = document.migrate().unwrap();
        assert_eq!(migrated.schema_version, DOCUMENT_SCHEMA_VERSION);
        assert!(migrated.references.is_empty());
    }

    #[test]
    fn unsupported_document_schema_is_rejected() {
        let mut document = document();
        document.schema_version = 1;
        assert!(document.migrate().is_err());
    }

    #[test]
    fn reference_geometry_and_paths_are_validated() {
        let mut document = document();
        document.references.push(ReferenceManifest {
            id: 1,
            file: "references/1.png".to_owned(),
            position: [-100.0, 20.0],
            size: [640.0, 480.0],
            visible: true,
            locked: false,
        });
        assert!(document.validate().is_ok());

        document.references[0].size[0] = 0.0;
        assert!(document.validate().is_err());
        document.references[0].size[0] = 640.0;
        document.references[0].file = "../reference.png".to_owned();
        assert!(document.validate().is_err());
    }

    #[test]
    fn generated_layer_paths_cannot_escape_revision() {
        let mut document = document();
        document.layers[0].file = "../outside.png".to_owned();
        assert!(document.validate().is_err());
    }

    #[test]
    fn titles_are_trimmed_and_empty_titles_are_rejected() {
        assert_eq!(normalized_title("  Study  ").unwrap(), "Study");
        assert!(normalized_title("  ").is_err());
    }
}
