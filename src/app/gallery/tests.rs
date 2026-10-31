use std::sync::Arc;

use super::*;

#[test]
fn stale_thumbnail_completion_is_ignored() {
    let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
    let id = ArtworkId::new();
    let current_path = PathBuf::from("current.png");
    assert!(
        thumbnail_sender
            .send(ThumbnailCompletion {
                id: id.clone(),
                path: PathBuf::from("stale.png"),
                result: Err("stale".to_owned()),
            })
            .is_ok()
    );
    assert!(
        thumbnail_sender
            .send(ThumbnailCompletion {
                id: id.clone(),
                path: current_path.clone(),
                result: Ok(image::RgbaImage::new(1, 1)),
            })
            .is_ok()
    );
    let controller = GalleryController {
        store: None,
        artworks: Vec::new(),
        warnings: Vec::new(),
        duplicate_receiver: None,
        load_receiver: None,
        load_started_at: None,
        thumbnail_sender,
        thumbnail_receiver,
        thumbnail_paths: vec![(id, current_path.clone())],
        wake: Arc::new(|| {}),
    };

    assert_eq!(
        controller
            .take_thumbnail_completion()
            .map(|completion| completion.path),
        Some(current_path)
    );
}
