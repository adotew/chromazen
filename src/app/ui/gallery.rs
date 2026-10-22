use std::path::PathBuf;

use egui::containers::menu::MenuButton;

use crate::artwork::{ArtworkId, ArtworkSummary};

use super::super::{
    command::{AppCommand, GalleryCommand, NavigationCommand, UiCommand},
    gallery::ThumbnailCompletion,
};
use super::apply_menu_item_padding;

const HEADER_HEIGHT: f32 = 32.0;
// On macOS the header sits in the transparent titlebar.
#[cfg(target_os = "macos")]
const HEADER_TOP_SPACE: f32 = 4.0;
#[cfg(target_os = "macos")]
const TITLE_INSET: f32 = 64.0;
#[cfg(target_os = "macos")]
const TITLE_SIZE: f32 = 20.0;
#[cfg(not(target_os = "macos"))]
const HEADER_TOP_SPACE: f32 = 40.0;
#[cfg(not(target_os = "macos"))]
const TITLE_INSET: f32 = 0.0;
#[cfg(not(target_os = "macos"))]
const TITLE_SIZE: f32 = 28.0;

#[derive(Default)]
pub(super) struct GalleryUi {
    thumbnails: Vec<Thumbnail>,
    rename: Option<(ArtworkId, String)>,
    delete: Option<(ArtworkId, String)>,
}

struct Thumbnail {
    id: ArtworkId,
    path: PathBuf,
    texture: egui::TextureHandle,
    content_uv: egui::Rect,
    content_aspect: f32,
}

impl GalleryUi {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        artworks: &[ArtworkSummary],
        warning: Option<&str>,
        commands: &mut Vec<AppCommand>,
    ) {
        self.sync_thumbnails(artworks);
        let top_margin = if cfg!(target_os = "macos") { 0 } else { 8 };
        let panel_frame = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: top_margin,
            bottom: 8,
        });
        egui::CentralPanel::default()
            .frame(panel_frame)
            .show_inside(ui, |ui| {
                ui.add_space(HEADER_TOP_SPACE);
                let (header_rect, header) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), HEADER_HEIGHT),
                    egui::Sense::click_and_drag(),
                );
                if cfg!(target_os = "macos") && header.drag_started_by(egui::PointerButton::Primary)
                {
                    commands.push(AppCommand::Ui(UiCommand::DragWindow));
                }
                ui.painter().text(
                    header_rect.left_center() + egui::vec2(TITLE_INSET, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "Chromazen",
                    egui::FontId::new(TITLE_SIZE, egui::FontFamily::Name("heming".into())),
                    ui.visuals().strong_text_color(),
                );
                let add_icon =
                    egui::Image::new(egui::include_image!("../../../assets/icons/plus.svg"))
                        .fit_to_exact_size(egui::Vec2::splat(18.0))
                        .alt_text("New artwork");
                let add_button = egui::Button::image(add_icon)
                    .frame(false)
                    .image_tint_follows_text_color(true);
                let add_rect = egui::Rect::from_min_size(
                    egui::pos2(header_rect.right() - HEADER_HEIGHT, header_rect.top()),
                    egui::Vec2::splat(HEADER_HEIGHT),
                );
                if ui
                    .put(add_rect, add_button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("New artwork")
                    .clicked()
                {
                    commands.push(AppCommand::Navigation(NavigationCommand::NewArtwork));
                }
                if let Some(warning) = warning {
                    ui.add_space(8.0);
                    ui.colored_label(egui::Color32::LIGHT_RED, warning);
                }
                ui.add_space(48.0);

                if artworks.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.heading("No artwork yet");
                        ui.label("Use the + button to begin painting.");
                    });
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let row = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
                    ui.with_layout(row, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(32.0, 10.0);
                        for artwork in artworks {
                            ui.allocate_ui_with_layout(
                                egui::vec2(220.0, 250.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::NONE.inner_margin(0).show(ui, |ui| {
                                        ui.set_width(198.0);
                                        let (open, content) =
                                            show_artwork_thumbnail(ui, self.thumbnail(&artwork.id));
                                        open.context_menu(|ui| {
                                            self.show_artwork_menu(ui, artwork, commands);
                                        });
                                        if open.clicked() {
                                            commands.push(AppCommand::Navigation(
                                                NavigationCommand::OpenArtwork(artwork.id.clone()),
                                            ));
                                        }
                                        self.show_artwork_menu_button(
                                            ui, content, artwork, commands,
                                        );
                                        ui.add_space(8.0);
                                        ui.vertical_centered(|ui| {
                                            ui.strong(&artwork.title);
                                            ui.label(format_dimensions(artwork.dimensions));
                                        });
                                    });
                                },
                            );
                        }
                    });
                });
            });
        self.show_rename_dialog(ui.ctx(), commands);
        self.show_delete_dialog(ui.ctx(), commands);
    }

    /// A hover-revealed menu button overlaid on the thumbnail's top-right corner.
    fn show_artwork_menu_button(
        &mut self,
        ui: &mut egui::Ui,
        content: egui::Rect,
        artwork: &ArtworkSummary,
        commands: &mut Vec<AppCommand>,
    ) {
        const SIZE: f32 = 28.0;
        const ICON_SIZE: f32 = 16.0;
        let rect = egui::Rect::from_min_size(
            content.right_top() + egui::vec2(-SIZE - 8.0, 8.0),
            egui::Vec2::splat(SIZE),
        );
        let mut overlay = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(&artwork.id)
                .max_rect(rect)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::TopDown,
                )),
        );
        // Keep the button visible while its menu is open, even after the pointer leaves the card.
        let menu_open = egui::Popup::is_id_open(ui.ctx(), overlay.next_auto_id().with("popup"));
        let opacity = ui.ctx().animate_bool(
            overlay.id().with("menu fade"),
            menu_open || ui.rect_contains_pointer(content),
        );
        if opacity == 0.0 {
            return;
        }
        overlay.multiply_opacity(opacity);
        // The app-wide 8pt horizontal button padding would make this wider than tall.
        overlay.spacing_mut().button_padding = egui::Vec2::splat((SIZE - ICON_SIZE) / 2.0);
        let icon = egui::Image::new(egui::include_image!(
            "../../../assets/icons/ellipsis-vertical.svg"
        ))
        .fit_to_exact_size(egui::Vec2::splat(ICON_SIZE))
        .tint(egui::Color32::WHITE)
        .alt_text("Artwork menu");
        let (menu, _) = MenuButton::from_button(
            egui::Button::image(icon)
                .fill(egui::Color32::from_black_alpha(140))
                .corner_radius(8)
                .min_size(egui::Vec2::splat(SIZE)),
        )
        .ui(&mut overlay, |ui| {
            self.show_artwork_menu(ui, artwork, commands);
        });
        menu.on_hover_text("Artwork menu");
    }

    fn sync_thumbnails(&mut self, artworks: &[ArtworkSummary]) {
        self.thumbnails.retain(|thumbnail| {
            artworks.iter().any(|artwork| {
                artwork.id == thumbnail.id && artwork.thumbnail_path == thumbnail.path
            })
        });
    }

    pub(super) fn apply_thumbnail(
        &mut self,
        context: &egui::Context,
        completion: ThumbnailCompletion,
    ) {
        self.thumbnails
            .retain(|thumbnail| thumbnail.id != completion.id);
        let image = match completion.result {
            Ok(image) => image,
            Err(error) => {
                log::warn!("{error}");
                return;
            }
        };
        let (content_uv, content_aspect) = thumbnail_content_bounds(&image);
        let size = [image.width() as usize, image.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let texture = context.load_texture(
            format!("artwork thumbnail {}", completion.id.as_str()),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.thumbnails.push(Thumbnail {
            id: completion.id,
            path: completion.path,
            texture,
            content_uv,
            content_aspect,
        });
    }

    fn thumbnail(&self, id: &ArtworkId) -> Option<&Thumbnail> {
        self.thumbnails.iter().find(|thumbnail| &thumbnail.id == id)
    }

    fn show_rename_dialog(&mut self, context: &egui::Context, commands: &mut Vec<AppCommand>) {
        let Some((id, title)) = self.rename.as_mut() else {
            return;
        };
        let mut close = false;
        egui::Window::new("Rename Artwork")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                let response = ui.text_edit_singleline(title);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    let submit = ui
                        .add_enabled(!title.trim().is_empty(), egui::Button::new("Rename"))
                        .clicked()
                        || (response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            && !title.trim().is_empty());
                    if submit {
                        commands.push(AppCommand::Gallery(GalleryCommand::Rename {
                            id: id.clone(),
                            title: title.clone(),
                        }));
                        close = true;
                    }
                });
            });
        if close {
            self.rename = None;
        }
    }

    fn show_delete_dialog(&mut self, context: &egui::Context, commands: &mut Vec<AppCommand>) {
        let Some((id, title)) = self.delete.as_ref() else {
            return;
        };
        let id = id.clone();
        let title = title.clone();
        let mut close = false;
        egui::Window::new("Delete Artwork?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(context, |ui| {
                ui.label(format!("Delete “{title}” permanently?"));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(egui::Color32::LIGHT_RED))
                        .clicked()
                    {
                        commands.push(AppCommand::Gallery(GalleryCommand::Delete(id.clone())));
                        close = true;
                    }
                });
            });
        if close {
            self.delete = None;
        }
    }

    fn show_artwork_menu(
        &mut self,
        ui: &mut egui::Ui,
        artwork: &ArtworkSummary,
        commands: &mut Vec<AppCommand>,
    ) {
        apply_menu_item_padding(ui);
        if ui.button("Rename").clicked() {
            self.rename = Some((artwork.id.clone(), artwork.title.clone()));
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            commands.push(AppCommand::Gallery(GalleryCommand::Duplicate(
                artwork.id.clone(),
            )));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.delete = Some((artwork.id.clone(), artwork.title.clone()));
            ui.close();
        }
    }
}

fn format_dimensions(dimensions: [u32; 2]) -> String {
    format!("{} \u{00d7} {}", dimensions[0], dimensions[1])
}

/// Returns the click response and the rect the artwork occupies within its square slot.
fn show_artwork_thumbnail(
    ui: &mut egui::Ui,
    thumbnail: Option<&Thumbnail>,
) -> (egui::Response, egui::Rect) {
    const SIZE: f32 = 198.0;
    let slot_size = egui::Vec2::splat(SIZE);
    let (slot, response) = ui.allocate_exact_size(slot_size, egui::Sense::click());
    let Some(thumbnail) = thumbnail else {
        egui::Image::new(egui::include_image!("../../../assets/icons/paintbrush.svg"))
            .tint(ui.visuals().weak_text_color())
            .alt_text("Open artwork")
            .paint_at(ui, slot);
        return (
            response.on_hover_cursor(egui::CursorIcon::PointingHand),
            slot,
        );
    };

    let content_size = if thumbnail.content_aspect >= 1.0 {
        egui::vec2(SIZE, SIZE / thumbnail.content_aspect)
    } else {
        egui::vec2(SIZE * thumbnail.content_aspect, SIZE)
    };
    let content = egui::Rect::from_center_size(slot.center(), content_size);
    egui::Image::new((thumbnail.texture.id(), content_size))
        .uv(thumbnail.content_uv)
        .corner_radius(20)
        .alt_text("Open artwork")
        .paint_at(ui, content);
    (
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        content,
    )
}

fn thumbnail_content_bounds(image: &image::RgbaImage) -> (egui::Rect, f32) {
    let mut min_x = image.width();
    let mut min_y = image.height();
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] == 0 {
            continue;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }

    if !found {
        return (
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
            1.0,
        );
    }
    let content_width = max_x - min_x + 1;
    let content_height = max_y - min_y + 1;
    let image_width = image.width() as f32;
    let image_height = image.height() as f32;
    (
        egui::Rect::from_min_max(
            egui::pos2(min_x as f32 / image_width, min_y as f32 / image_height),
            egui::pos2(
                (max_x + 1) as f32 / image_width,
                (max_y + 1) as f32 / image_height,
            ),
        ),
        content_width as f32 / content_height as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
