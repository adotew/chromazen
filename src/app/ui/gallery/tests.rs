use super::*;

#[test]
fn gallery_thumbnails_grow_until_another_column_fits() {
    assert_eq!(gallery_columns(100.0), (1, MIN_THUMBNAIL_SIZE));
    assert_eq!(
        gallery_columns(2.0 * MIN_THUMBNAIL_SIZE + COLUMN_GAP + 100.0),
        (2, MIN_THUMBNAIL_SIZE + 50.0)
    );
    assert_eq!(
        gallery_columns(3.0 * MIN_THUMBNAIL_SIZE + 2.0 * COLUMN_GAP),
        (3, MIN_THUMBNAIL_SIZE)
    );
}

#[test]
fn rename_dialog_opens_focused_with_the_title_selected() {
    let context = egui::Context::default();
    let mut gallery = GalleryUi {
        rename: Some((ArtworkId::new(), "Untitled".to_owned(), true)),
        ..Default::default()
    };
    let mut commands = Vec::new();
    // The menu click that opened the dialog arrives in the dialog's first frame.
    let click = egui::pos2(5.0, 5.0);
    let opening_click = egui::RawInput {
        events: [true, false]
            .map(|pressed| egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            })
            .to_vec(),
        ..Default::default()
    };
    for input in [opening_click, egui::RawInput::default()] {
        let _ = context.run_ui(input, |ui| {
            gallery.show_rename_dialog(ui.ctx(), &mut commands);
        });
    }

    let focused = context.memory(|memory| memory.focused());
    let range = focused
        .and_then(|id| egui::text_edit::TextEditState::load(&context, id))
        .and_then(|state| state.cursor.char_range())
        .map(|range| (range.primary.index, range.secondary.index));
    assert!(matches!(range, Some((0, 8)) | Some((8, 0))), "{range:?}");
}

#[test]
fn transparent_bars_are_excluded_from_thumbnail_bounds() {
    let mut image = image::RgbaImage::new(4, 4);
    for y in 1..=2 {
        for x in 0..4 {
            image.put_pixel(x, y, image::Rgba([1, 2, 3, 255]));
        }
    }

    let (uv, aspect) = thumbnail_content_bounds(&image);
    assert_eq!(uv.min, egui::pos2(0.0, 0.25));
    assert_eq!(uv.max, egui::pos2(1.0, 0.75));
    assert_eq!(aspect, 2.0);
}
