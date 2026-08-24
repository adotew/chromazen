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

use super::format::{DocumentManifest, PROJECT_SCHEMA_VERSION, ProjectManifest, normalized_title};

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
    pub(crate) dimensions: [u32; 2],
    pub(crate) thumbnail_path: PathBuf,
}

#[derive(Default)]
pub(crate) struct ArtworkCatalog {
    pub(crate) artworks: Vec<ArtworkSummary>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) enum LayerSource {
    Png(Vec<u8>),
    Copy(PathBuf),
    ReuseCurrent,
}

pub(crate) struct LayerWrite {
    pub(crate) id: u64,
    pub(crate) source: LayerSource,
}

pub(crate) enum ReferenceSource {
    Png(Vec<u8>),
    Copy(PathBuf),
    ReuseCurrent,
}

pub(crate) struct ReferenceWrite {
    pub(crate) id: u64,
    pub(crate) source: ReferenceSource,
}

pub(crate) struct RevisionWrite {
    pub(crate) document: DocumentManifest,
    pub(crate) layers: Vec<LayerWrite>,
    pub(crate) references: Vec<ReferenceWrite>,
    pub(crate) thumbnail_png: Vec<u8>,
}

pub(crate) struct LoadedArtwork {
    pub(crate) summary: ArtworkSummary,
    pub(crate) document: DocumentManifest,
    pub(crate) layer_paths: Vec<PathBuf>,
    pub(crate) reference_paths: Vec<PathBuf>,
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

    pub(crate) fn scan_catalog(&self) -> ArtworkCatalog {
        let mut catalog = ArtworkCatalog::default();
        self.cleanup_abandoned_writes();
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
                let document_path = revision.join(DOCUMENT_FILE);
                let source = fs::read_to_string(&document_path)
                    .map_err(|error| ArtworkError::io("read", &document_path, error))?;
                let document: DocumentManifest = toml::from_str(&source).map_err(|error| {
                    ArtworkError::new(format!(
                        "failed to parse {}: {error}",
                        document_path.display()
                    ))
                })?;
                self.cleanup_old_revisions(&id, project.current_revision);
                Ok(ArtworkSummary {
                    id,
                    title: project.title,
                    modified_unix_ms: project.modified_unix_ms,
                    dimensions: [document.width, document.height],
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
        let document: DocumentManifest = toml::from_str::<DocumentManifest>(&source)
            .map_err(|error| {
                ArtworkError::new(format!(
                    "failed to parse {}: {error}",
                    document_path.display()
                ))
            })?
            .migrate()
            .map_err(|error| {
                ArtworkError::new(format!(
                    "failed to migrate {}: {error}",
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
        let reference_paths = document
            .references
            .iter()
            .map(|reference| revision.join(&reference.file))
            .collect();
        Ok(LoadedArtwork {
            summary: ArtworkSummary {
                id: id.clone(),
                title: project.title,
                modified_unix_ms: project.modified_unix_ms,
                dimensions: [document.width, document.height],
                thumbnail_path: revision.join(THUMBNAIL_FILE),
            },
            document,
            layer_paths,
            reference_paths,
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
            if !write.document.references.is_empty() {
                fs::create_dir_all(temporary.join("references"))
                    .map_err(|error| ArtworkError::io("create", &temporary, error))?;
            }
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
                    LayerSource::Copy(source) => {
                        fs::copy(source, &destination)
                            .map_err(|error| ArtworkError::io("copy", source, error))?;
                    }
                    LayerSource::ReuseCurrent => {
                        let source = current_revision
                            .as_ref()
                            .ok_or_else(|| {
                                ArtworkError::new("cannot reuse a layer in the first revision")
                            })?
                            .join(&layer.file);
                        reuse_file(&source, &destination)?;
                    }
                }
            }
            for reference in &write.document.references {
                let reference_write = write
                    .references
                    .iter()
                    .find(|candidate| candidate.id == reference.id)
                    .ok_or_else(|| {
                        ArtworkError::new(format!("missing image for reference {}", reference.id))
                    })?;
                let destination = temporary.join(&reference.file);
                match &reference_write.source {
                    ReferenceSource::Png(contents) => fs::write(&destination, contents)
                        .map_err(|error| ArtworkError::io("write", &destination, error))?,
                    ReferenceSource::Copy(source) => {
                        fs::copy(source, &destination)
                            .map_err(|error| ArtworkError::io("copy", source, error))?;
                    }
                    ReferenceSource::ReuseCurrent => {
                        let source = current_revision
                            .as_ref()
                            .ok_or_else(|| {
                                ArtworkError::new("cannot reuse a reference in the first revision")
                            })?
                            .join(&reference.file);
                        reuse_file(&source, &destination)?;
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
            let metadata_result = write_toml_atomically(&artwork_dir.join(PROJECT_FILE), &project);
            self.resolve_project_commit_result(id, &project, &final_revision, metadata_result)?;
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
            dimensions: [write.document.width, write.document.height],
            thumbnail_path: final_revision.join(THUMBNAIL_FILE),
        })
    }

    pub(crate) fn duplicate(&self, id: &ArtworkId) -> Result<ArtworkSummary, ArtworkError> {
        let loaded = self.load(id)?;
        let thumbnail_png = fs::read(&loaded.summary.thumbnail_path)
            .map_err(|error| ArtworkError::io("read", &loaded.summary.thumbnail_path, error))?;
        let layers = loaded
            .document
            .layers
            .iter()
            .zip(loaded.layer_paths)
            .map(|(layer, path)| LayerWrite {
                id: layer.id,
                source: LayerSource::Copy(path),
            })
            .collect();
        let references = loaded
            .document
            .references
            .iter()
            .zip(loaded.reference_paths)
            .map(|(reference, path)| ReferenceWrite {
                id: reference.id,
                source: ReferenceSource::Copy(path),
            })
            .collect();
        self.commit_revision(
            &ArtworkId::new(),
            &format!("{} copy", loaded.summary.title),
            RevisionWrite {
                document: loaded.document,
                layers,
                references,
                thumbnail_png,
            },
        )
    }

    pub(crate) fn rename(&self, id: &ArtworkId, title: &str) -> Result<(), ArtworkError> {
        let mut project = self.read_project(id)?;
        project.title = normalized_title(title).map_err(ArtworkError::new)?;
        project.modified_unix_ms = now_unix_ms();
        write_toml_atomically(&self.artwork_path(id).join(PROJECT_FILE), &project)
    }

    pub(crate) fn delete(&self, id: &ArtworkId) -> Result<(), ArtworkError> {
        let source = self.artwork_path(id);
        let trash = self.root.join(format!(".trash-{}", Uuid::new_v4()));
        fs::rename(&source, &trash)
            .map_err(|error| ArtworkError::io("move for deletion", &source, error))?;
        fs::remove_dir_all(&trash).map_err(|error| ArtworkError::io("delete", &trash, error))
    }

    fn resolve_project_commit_result(
        &self,
        id: &ArtworkId,
        project: &ProjectManifest,
        revision: &Path,
        result: Result<(), ArtworkError>,
    ) -> Result<(), ArtworkError> {
        let Err(commit_error) = result else {
            return Ok(());
        };
        if self
            .read_project(id)
            .is_ok_and(|committed| committed == *project)
        {
            return Ok(());
        }
        fs::remove_dir_all(revision).map_err(|cleanup_error| {
            ArtworkError::new(format!(
                "{commit_error}; also failed to remove uncommitted revision {}: {cleanup_error}",
                revision.display()
            ))
        })?;
        Err(commit_error)
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

    fn cleanup_abandoned_writes(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(".trash-") {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    fn cleanup_old_revisions(&self, id: &ArtworkId, current_revision: u64) {
        let revisions = self.artwork_path(id).join("revisions");
        let Ok(entries) = fs::read_dir(revisions) else {
            return;
        };
        let current_name = format!("{current_revision:016}");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".tmp-")
                || (name.len() == 16
                    && name.bytes().all(|byte| byte.is_ascii_digit())
                    && name != current_name)
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
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

fn reuse_file(source: &Path, destination: &Path) -> Result<(), ArtworkError> {
    if fs::hard_link(source, destination).is_err() {
        fs::copy(source, destination).map_err(|error| ArtworkError::io("copy", source, error))?;
    }
    Ok(())
}

fn write_toml_atomically(path: &Path, value: &impl serde::Serialize) -> Result<(), ArtworkError> {
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
    use crate::artwork::format::{DOCUMENT_SCHEMA_VERSION, LayerManifest, ReferenceManifest};

    fn revision(pixel: u8) -> RevisionWrite {
        RevisionWrite {
            document: DocumentManifest {
                schema_version: DOCUMENT_SCHEMA_VERSION,
                width: 1,
                height: 1,
                background: [255; 3],
                brush_color: [170, 187, 204, 255],
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
            },
            layers: vec![LayerWrite {
                id: 1,
                source: LayerSource::Png(vec![pixel]),
            }],
            references: Vec::new(),
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
        assert_eq!(store.scan_catalog().artworks.len(), 1);
    }

    #[test]
    fn catalog_and_load_summary_include_dimensions() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "Untitled", revision(1)).unwrap();

        let catalog = store.scan_catalog();
        assert_eq!(catalog.artworks[0].dimensions, [1, 1]);

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.summary.dimensions, [1, 1]);
    }

    #[test]
    fn reference_assets_round_trip_and_can_be_reused() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        let mut first = revision(1);
        first.document.references.push(ReferenceManifest {
            id: 7,
            file: "references/7.png".to_owned(),
            position: [4100.0, 20.0],
            size: [640.0, 480.0],
            visible: true,
            locked: false,
        });
        first.references.push(ReferenceWrite {
            id: 7,
            source: ReferenceSource::Png(vec![4, 5, 6]),
        });
        store.commit_revision(&id, "Study", first).unwrap();

        let mut second = revision(2);
        second.document.references.push(ReferenceManifest {
            id: 7,
            file: "references/7.png".to_owned(),
            position: [4200.0, 40.0],
            size: [640.0, 480.0],
            visible: true,
            locked: true,
        });
        second.references.push(ReferenceWrite {
            id: 7,
            source: ReferenceSource::ReuseCurrent,
        });
        store.commit_revision(&id, "Study", second).unwrap();

        let loaded = store.load(&id).unwrap();
        assert_eq!(loaded.document.references[0].position, [4200.0, 40.0]);
        assert!(loaded.document.references[0].locked);
        assert_eq!(fs::read(&loaded.reference_paths[0]).unwrap(), [4, 5, 6]);
    }

    #[test]
    fn duplicate_copies_document_assets_under_a_new_id() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "Study", revision(7)).unwrap();

        let duplicate = store.duplicate(&id).unwrap();
        let loaded = store.load(&duplicate.id).unwrap();

        assert_ne!(duplicate.id, id);
        assert_eq!(duplicate.title, "Study copy");
        assert_eq!(loaded.document, store.load(&id).unwrap().document);
        assert_eq!(fs::read(&loaded.layer_paths[0]).unwrap(), [7]);
        assert_eq!(fs::read(&loaded.summary.thumbnail_path).unwrap(), [7]);
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
        assert_eq!(store.scan_catalog().artworks.len(), 2);
    }

    #[test]
    fn rename_and_delete_update_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "Untitled", revision(1)).unwrap();
        store.rename(&id, "Study").unwrap();
        assert_eq!(store.scan_catalog().artworks[0].title, "Study");
        store.delete(&id).unwrap();
        assert!(store.scan_catalog().artworks.is_empty());
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

        let catalog = store.scan_catalog();
        assert_eq!(catalog.artworks.len(), 1);
        assert_eq!(catalog.warnings.len(), 1);
    }

    #[test]
    fn empty_rename_is_rejected_without_changing_the_title() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "Untitled", revision(1)).unwrap();

        assert!(store.rename(&id, "  ").is_err());
        assert_eq!(store.load(&id).unwrap().summary.title, "Untitled");
    }

    #[test]
    fn discovery_removes_abandoned_trash_directories() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let trash = temp.path().join(".trash-abandoned");
        fs::create_dir_all(&trash).unwrap();
        fs::write(trash.join("pixels"), b"data").unwrap();

        store.scan_catalog();

        assert!(!trash.exists());
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

    #[test]
    fn failed_project_commit_removes_revision_before_retry() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        store.commit_revision(&id, "One", revision(1)).unwrap();
        let mut next_project = store.read_project(&id).unwrap();
        next_project.current_revision = 2;
        next_project.title = "Two".to_owned();
        let uncommitted = store.revision_path(&id, 2);
        fs::create_dir_all(&uncommitted).unwrap();

        assert!(
            store
                .resolve_project_commit_result(
                    &id,
                    &next_project,
                    &uncommitted,
                    Err(ArtworkError::new("injected metadata failure")),
                )
                .is_err()
        );
        assert!(!uncommitted.exists());

        store.commit_revision(&id, "Two", revision(2)).unwrap();
        assert_eq!(store.load(&id).unwrap().summary.title, "Two");
    }

    #[test]
    fn failed_first_project_commit_removes_revision_before_retry() {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtworkStore::from_root(temp.path());
        let id = ArtworkId::new();
        let uncommitted = store.revision_path(&id, 1);
        fs::create_dir_all(&uncommitted).unwrap();
        let project = ProjectManifest {
            schema_version: PROJECT_SCHEMA_VERSION,
            id: id.as_str().to_owned(),
            title: "Untitled".to_owned(),
            current_revision: 1,
            modified_unix_ms: 1,
        };

        assert!(
            store
                .resolve_project_commit_result(
                    &id,
                    &project,
                    &uncommitted,
                    Err(ArtworkError::new("injected metadata failure")),
                )
                .is_err()
        );
        assert!(!uncommitted.exists());

        store.commit_revision(&id, "Untitled", revision(1)).unwrap();
        assert_eq!(store.load(&id).unwrap().summary.title, "Untitled");
    }
}
