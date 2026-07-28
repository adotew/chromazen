use serde::{Deserialize, Serialize};

pub(crate) const PROJECT_SCHEMA_VERSION: u32 = 1;
pub(crate) const DOCUMENT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectManifest {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) current_revision: u64,
    pub(crate) modified_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentManifest {
    pub(crate) schema_version: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) background: [u8; 3],
    pub(crate) selected_layer: u64,
    pub(crate) layers: Vec<LayerManifest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LayerManifest {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) visible: bool,
    pub(crate) opacity: u8,
    pub(crate) file: String,
}

impl DocumentManifest {
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
                file: "layers/1.png".to_owned(),
            }],
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
    fn invalid_opacity_is_rejected() {
        let mut document = document();
        document.layers[0].opacity = 101;
        assert!(document.validate().is_err());
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
    fn old_document_schema_is_rejected() {
        let mut document = document();
        document.schema_version = 1;
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
