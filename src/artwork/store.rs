use std::{
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use atomic_write_file::AtomicWriteFile;
use directories::ProjectDirs;
use uuid::Uuid;

use super::format::{
    DOCUMENT_SCHEMA_VERSION, DocumentManifest, PROJECT_SCHEMA_VERSION, ProjectManifest,
    normalized_title,
};

const APP_NAME: &str = "chromazen";
const PROJECT_FILE: &str = "project.toml";
const DOCUMENT_FILE: &str = "document.toml";
const THUMBNAIL_FILE: &str = "thumbnail.png";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ArtworkId(String);

impl ArtworkId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub(crate) fn parse(value: impl Into<String>) -> Result<Self, ArtworkError> {
        let value = value.into();
        Uuid::parse_str(&value)
            .map_err(|_| ArtworkError::new(format!("invalid artwork ID '{value}'")))?;
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ArtworkSummary {
    pub(crate) id: ArtworkId,
    pub(crate) title: String,
    pub(crate) modified_unix_ms: u64,
    pub(crate) thumbnail_path: PathBuf,
}

#[derive(Default)]
pub(crate) struct ArtworkCatalog {
    pub(crate) artworks: Vec<ArtworkSummary>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) enum LayerSource {
    Png(Vec<u8>),
    ReuseCurrent,
}

pub(crate) struct LayerWrite {
    pub(crate) id: u64,
    pub(crate) source: LayerSource,
}

pub(crate) struct RevisionWrite {
    pub(crate) document: DocumentManifest,
    pub(crate) layers: Vec<LayerWrite>,
    pub(crate) thumbnail_png: Vec<u8>,
}

pub(crate) struct LoadedArtwork {
    pub(crate) summary: ArtworkSummary,
    pub(crate) document: DocumentManifest,
    pub(crate) layer_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct ArtworkStore {
    root: PathBuf,
}

impl ArtworkStore {
    pub(crate) fn discover() -> Result<Self, ArtworkError> {
        let dirs = ProjectDirs::from("", "", APP_NAME)
            .ok_or_else(|| ArtworkError::new("could not determine application data directory"))?;
        Ok(Self::from_root(dirs.data_dir().join("artworks")))
    }

    pub(crate) fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn catalog(&self) -> ArtworkCatalog {
        let mut catalog = ArtworkCatalog::default();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return catalog,
            Err(error) => {
                catalog.warnings.push(
                    ArtworkError::io("read artwork directory", &self.root, error).to_string(),
                );
                return catalog;
            }
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(id) = ArtworkId::parse(name) else {
                continue;
            };
            match self.read_project(&id).and_then(|project| {
                self.validate_project(&id, &project)?;
                let revision = self.revision_path(&id, project.current_revision);
                if !revision.join(DOCUMENT_FILE).is_file()
                    || !revision.join(THUMBNAIL_FILE).is_file()
                {
                    return Err(ArtworkError::new(format!(
                        "artwork '{}' points to an incomplete revision",
                        project.title
                    )));
                }
                Ok(ArtworkSummary {
                    id,
                    title: project.title,
                    modified_unix_ms: project.modified_unix_ms,
                    thumbnail_path: revision.join(THUMBNAIL_FILE),
                })
            }) {
                Ok(summary) => catalog.artworks.push(summary),
                Err(error) => catalog.warnings.push(error.to_string()),
            }
        }
        catalog.artworks.sort_by(|left, right| {
            right
                .modified_unix_ms
                .cmp(&left.modified_unix_ms)
                .then_with(|| left.title.cmp(&right.title))
        });
        catalog
    }

    pub(crate) fn load(&self, id: &ArtworkId) -> Result<LoadedArtwork, ArtworkError> {
        let project = self.read_project(id)?;
        self.validate_project(id, &project)?;
        let revision = self.revision_path(id, project.current_revision);
        let document_path = revision.join(DOCUMENT_FILE);
        let source = fs::read_to_string(&document_path)
            .map_err(|error| ArtworkError::io("read", &document_path, error))?;
        let document: DocumentManifest = toml::from_str(&source).map_err(|error| {
            ArtworkError::new(format!(
                "failed to parse {}: {error}",
                document_path.display()
            ))
        })?;
        document.validate().map_err(|error| {
            ArtworkError::new(format!(
                "invalid document in {}: {error}",
                document_path.display()
            ))
        })?;
        let layer_paths = document
            .layers
            .iter()
            .map(|layer| revision.join(&layer.file))
            .collect();
        Ok(LoadedArtwork {
            summary: ArtworkSummary {
                id: id.clone(),
                title: project.title,
                modified_unix_ms: project.modified_unix_ms,
                thumbnail_path: revision.join(THUMBNAIL_FILE),
            },
            document,
            layer_paths,
        })
    }

    pub(crate) fn commit_revision(
        &self,
        id: &ArtworkId,
        title: &str,
        write: RevisionWrite,
    ) -> Result<ArtworkSummary, ArtworkError> {
        let title = normalized_title(title).map_err(ArtworkError::new)?;
        write.document.validate().map_err(ArtworkError::new)?;
        fs::create_dir_all(&self.root)
            .map_err(|error| ArtworkError::io("create", &self.root, error))?;
        let artwork_dir = self.artwork_path(id);
        let revisions_dir = artwork_dir.join("revisions");
        fs::create_dir_all(&revisions_dir)
            .map_err(|error| ArtworkError::io("create", &revisions_dir, error))?;

        let previous = self.read_project(id).ok();
        let next_revision = previous
            .as_ref()
            .map_or(1, |project| project.current_revision.saturating_add(1));
        let temporary = revisions_dir.join(format!(".tmp-{}", Uuid::new_v4()));
        let final_revision = self.revision_path(id, next_revision);
        let result = (|| {
            fs::create_dir_all(temporary.join("layers"))
                .map_err(|error| ArtworkError::io("create", &temporary, error))?;
            let current_revision = previous
                .as_ref()
                .map(|project| self.revision_path(id, project.current_revision));
            for layer in &write.document.layers {
                let layer_write = write
                    .layers
                    .iter()
                    .find(|candidate| candidate.id == layer.id)
                    .ok_or_else(|| {
                        ArtworkError::new(format!("missing pixels for layer {}", layer.id))
                    })?;
                let destination = temporary.join(&layer.file);
                match &layer_write.source {
                    LayerSource::Png(contents) => fs::write(&destination, contents)
                        .map_err(|error| ArtworkError::io("write", &destination, error))?,
                    LayerSource::ReuseCurrent => {
                        let source = current_revision
                            .as_ref()
                            .ok_or_else(|| {
                                ArtworkError::new("cannot reuse a layer in the first revision")
                            })?
                            .join(&layer.file);
                        if fs::hard_link(&source, &destination).is_err() {
                            fs::copy(&source, &destination)
                                .map_err(|error| ArtworkError::io("copy", &source, error))?;
                        }
                    }
                }
            }
            let document_source = toml::to_string_pretty(&write.document).map_err(|error| {
                ArtworkError::new(format!("failed to serialize document: {error}"))
            })?;
            fs::write(temporary.join(DOCUMENT_FILE), document_source).map_err(|error| {
                ArtworkError::io("write", &temporary.join(DOCUMENT_FILE), error)
            })?;
            fs::write(temporary.join(THUMBNAIL_FILE), &write.thumbnail_png).map_err(|error| {
                ArtworkError::io("write", &temporary.join(THUMBNAIL_FILE), error)
            })?;
            fs::rename(&temporary, &final_revision)
                .map_err(|error| ArtworkError::io("commit revision", &final_revision, error))?;

            let project = ProjectManifest {
                schema_version: PROJECT_SCHEMA_VERSION,
                id: id.as_str().to_owned(),
                title: title.clone(),
                current_revision: next_revision,
                modified_unix_ms: now_unix_ms(),
            };
            atomic_toml(&artwork_dir.join(PROJECT_FILE), &project)?;
            Ok(project)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&temporary);
        }
        let project = result?;
        if let Some(previous) = previous {
            let _ = fs::remove_dir_all(self.revision_path(id, previous.current_revision));
        }
        Ok(ArtworkSummary {
            id: id.clone(),
            title: project.title,
            modified_unix_ms: project.modified_unix_ms,
            thumbnail_path: final_revision.join(THUMBNAIL_FILE),
        })
    }

    pub(crate) fn rename(&self, id: &ArtworkId, title: &str) -> Result<(), ArtworkError> {
        let mut project = self.read_project(id)?;
        project.title = normalized_title(title).map_err(ArtworkError::new)?;
        project.modified_unix_ms = now_unix_ms();
        atomic_toml(&self.artwork_path(id).join(PROJECT_FILE), &project)
    }

    pub(crate) fn delete(&self, id: &ArtworkId) -> Result<(), ArtworkError> {
        let source = self.artwork_path(id);
        let trash = self.root.join(format!(".trash-{}", Uuid::new_v4()));
        fs::rename(&source, &trash)
            .map_err(|error| ArtworkError::io("move for deletion", &source, error))?;
        fs::remove_dir_all(&trash).map_err(|error| ArtworkError::io("delete", &trash, error))
    }

    fn read_project(&self, id: &ArtworkId) -> Result<ProjectManifest, ArtworkError> {
        let path = self.artwork_path(id).join(PROJECT_FILE);
        let source =
            fs::read_to_string(&path).map_err(|error| ArtworkError::io("read", &path, error))?;
        toml::from_str(&source).map_err(|error| {
            ArtworkError::new(format!("failed to parse {}: {error}", path.display()))
        })
    }

    fn validate_project(
        &self,
        id: &ArtworkId,
        project: &ProjectManifest,
    ) -> Result<(), ArtworkError> {
        if project.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(ArtworkError::new(format!(
                "unsupported project schema_version {}; expected {PROJECT_SCHEMA_VERSION}",
                project.schema_version
            )));
        }
        if project.id != id.as_str() || project.current_revision == 0 {
            return Err(ArtworkError::new("invalid artwork project metadata"));
        }
        normalized_title(&project.title).map_err(ArtworkError::new)?;
        Ok(())
    }

    fn artwork_path(&self, id: &ArtworkId) -> PathBuf {
        self.root.join(id.as_str())
    }

    fn revision_path(&self, id: &ArtworkId, revision: u64) -> PathBuf {
        self.artwork_path(id)
            .join("revisions")
            .join(format!("{revision:016}"))
    }
}

fn atomic_toml(path: &Path, value: &impl serde::Serialize) -> Result<(), ArtworkError> {
    let source = toml::to_string_pretty(value)
        .map_err(|error| ArtworkError::new(format!("failed to serialize metadata: {error}")))?;
    let mut file = AtomicWriteFile::options()
        .open(path)
        .map_err(|error| ArtworkError::io("open for atomic writing", path, error))?;
    file.write_all(source.as_bytes())
        .map_err(|error| ArtworkError::io("write", path, error))?;
    file.flush()
        .map_err(|error| ArtworkError::io("flush", path, error))?;
    file.commit()
        .map_err(|error| ArtworkError::io("commit", path, error))
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[derive(Clone, Debug)]
pub(crate) struct ArtworkError(String);

impl ArtworkError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn io(operation: &str, path: &Path, error: std::io::Error) -> Self {
        Self(format!("failed to {operation} {}: {error}", path.display()))
    }
}

impl fmt::Display for ArtworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ArtworkError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork::format::LayerManifest;

    fn revision(pixel: u8) -> RevisionWrite {
        RevisionWrite {
            document: DocumentManifest {
                schema_version: DOCUMENT_SCHEMA_VERSION,
                width: 1,
                height: 1,
                background: [255; 3],
                selected_layer: 1,
                layers: vec![LayerManifest {
                    id: 1,
                    name: "Layer 1".to_owned(),
                    file: "layers/1.png".to_owned(),
                }],
            },
            layers: vec![LayerWrite {
                id: 1,
                source: LayerSource::Png(vec![pixel]),
            }],
            thumbnail_png: vec![pixel],
        }
    }

    #[test]
    fn revision_round_trips_and_catalog_uses_title() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "Untitled", revision(1)).unwrap();

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.summary.title, "Untitled");
        assert_eq!(loaded.document.layers[0].id, 1);
        assert_eq!(store.catalog().artworks.len(), 1);
    }

    #[test]
    fn duplicate_titles_are_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        store
            .commit_revision(&ArtworkId::new(), "Untitled", revision(1))
            .unwrap();
        store
            .commit_revision(&ArtworkId::new(), "Untitled", revision(2))
            .unwrap();
        assert_eq!(store.catalog().artworks.len(), 2);
    }

    #[test]
    fn rename_and_delete_update_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "Untitled", revision(1)).unwrap();
        store.rename(&id, "Study").unwrap();
        assert_eq!(store.catalog().artworks[0].title, "Study");
        store.delete(&id).unwrap();
        assert!(store.catalog().artworks.is_empty());
    }

    #[test]
    fn corrupt_artwork_does_not_hide_valid_artwork() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        store
            .commit_revision(&ArtworkId::new(), "Valid", revision(1))
            .unwrap();
        let corrupt = ArtworkId::new();
        fs::create_dir_all(store.artwork_path(&corrupt)).unwrap();
        fs::write(
            store.artwork_path(&corrupt).join(PROJECT_FILE),
            "not toml = [",
        )
        .unwrap();

        let catalog = store.catalog();
        assert_eq!(catalog.artworks.len(), 1);
        assert_eq!(catalog.warnings.len(), 1);
    }

    #[test]
    fn later_revision_replaces_previous_revision() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "One", revision(1)).unwrap();
        store.commit_revision(&id, "Two", revision(2)).unwrap();

        let project = store.read_project(&id).unwrap();
        assert_eq!(project.current_revision, 2);
        assert_eq!(store.load(&id).unwrap().summary.title, "Two");
        assert!(!store.revision_path(&id, 1).exists());
    }
}
