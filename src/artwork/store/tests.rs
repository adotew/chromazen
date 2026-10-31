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
            rotation: 0.0,
            flip: [false; 2],
            selected_layer: 1,
            layers: vec![LayerManifest {
                id: 1,
                name: "Layer 1".to_owned(),
                visible: true,
                opacity: 100,
                clipped: false,
                blend_mode: chromazen_canvas::BlendMode::Normal,
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
