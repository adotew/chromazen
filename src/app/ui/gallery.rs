use std::path::PathBuf;

use crate::artwork::{ArtworkId, ArtworkSummary};

use super::super::command::{AppCommand, GalleryCommand, NavigationCommand};
use super::apply_menu_item_padding;

#[derive(Default)]
pub(super) struct GalleryUi {
    thumbnails: Vec<Thumbnail>,
    failed_thumbnails: Vec<ArtworkId>,
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
        self.sync_thumbnails(ui.ctx(), artworks);
        let panel_frame =
            egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(24, 8));
        egui::CentralPanel::default()
            .frame(panel_frame)
            .show_inside(ui, |ui| {
                ui.add_space(40.0);
                let (header_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 32.0),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    header_rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    "Chromazen",
                    egui::FontId::new(28.0, egui::FontFamily::Name("elms_sans".into())),
                    ui.visuals().text_color(),
                );
                let add_icon =
                    egui::Image::new(egui::include_image!("../../../assets/icons/plus.svg"))
                        .fit_to_exact_size(egui::Vec2::splat(18.0))
                        .alt_text("New artwork");
                let add_button = egui::Button::image(add_icon)
                    .image_tint_follows_text_color(true)
                    .corner_radius(8);
                let add_rect = egui::Rect::from_min_size(
                    egui::pos2(header_rect.right() - 32.0, header_rect.top()),
                    egui::Vec2::splat(32.0),
                );
                if ui
                    .put(add_rect, add_button)
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
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(32.0, 10.0);
                        for artwork in artworks {
                            ui.allocate_ui_with_layout(
                                egui::vec2(220.0, 250.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::NONE.inner_margin(0).show(ui, |ui| {
                                        ui.set_width(198.0);
                                        let open =
                                            show_artwork_thumbnail(ui, self.thumbnail(&artwork.id));
                                        open.context_menu(|ui| {
                                            apply_menu_item_padding(ui);
                                            if ui.button("Rename").clicked() {
                                                self.rename = Some((
                                                    artwork.id.clone(),
                                                    artwork.title.clone(),
                                                ));
                                                ui.close();
                                            }
                                            if ui.button("Duplicate").clicked() {
                                                commands.push(AppCommand::Gallery(
                                                    GalleryCommand::Duplicate(artwork.id.clone()),
                                                ));
                                                ui.close();
                                            }
                                            if ui.button("Delete").clicked() {
                                                self.delete = Some((
                                                    artwork.id.clone(),
                                                    artwork.title.clone(),
                                                ));
                                                ui.close();
                                            }
                                        });
                                        if open.clicked() {
                                            commands.push(AppCommand::Navigation(
                                                NavigationCommand::OpenArtwork(artwork.id.clone()),
                                            ));
                                        }
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

    fn sync_thumbnails(&mut self, context: &egui::Context, artworks: &[ArtworkSummary]) {
        self.thumbnails
            .retain(|thumbnail| artworks.iter().any(|artwork| artwork.id == thumbnail.id));
        self.failed_thumbnails
            .retain(|id| artworks.iter().any(|artwork| artwork.id == *id));
        for artwork in artworks {
            let current = self
                .thumbnails
                .iter()
                .find(|thumbnail| thumbnail.id == artwork.id);
            if current.is_some_and(|thumbnail| thumbnail.path == artwork.thumbnail_path)
                || self.failed_thumbnails.contains(&artwork.id)
            {
                continue;
            }
            self.thumbnails
                .retain(|thumbnail| thumbnail.id != artwork.id);
            match image::open(&artwork.thumbnail_path) {
                Ok(image) => {
                    let image = image.to_rgba8();
                    let (content_uv, content_aspect) = thumbnail_content_bounds(&image);
                    let size = [image.width() as usize, image.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                    let texture = context.load_texture(
                        format!("artwork thumbnail {}", artwork.id.as_str()),
                        color,
                        egui::TextureOptions::LINEAR,
                    );
                    self.thumbnails.push(Thumbnail {
                        id: artwork.id.clone(),
                        path: artwork.thumbnail_path.clone(),
                        texture,
                        content_uv,
                        content_aspect,
                    });
                }
                Err(error) => {
                    log::warn!(
                        "failed to load artwork thumbnail {}: {error}",
                        artwork.thumbnail_path.display()
                    );
                    self.failed_thumbnails.push(artwork.id.clone());
                }
            }
        }
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
}

fn format_dimensions(dimensions: [u32; 2]) -> String {
    format!("{} \u{00d7} {}", dimensions[0], dimensions[1])
}

fn show_artwork_thumbnail(ui: &mut egui::Ui, thumbnail: Option<&Thumbnail>) -> egui::Response {
    const SIZE: f32 = 198.0;
    let slot_size = egui::Vec2::splat(SIZE);
    let Some(thumbnail) = thumbnail else {
        let image = egui::Image::new(egui::include_image!("../../../assets/icons/paintbrush.svg"))
            .tint(ui.visuals().weak_text_color())
            .corner_radius(12)
            .fit_to_exact_size(slot_size);
        return ui.add(egui::Button::image(image).frame(false));
    };

    let (slot, response) = ui.allocate_exact_size(slot_size, egui::Sense::click());
    let content_size = if thumbnail.content_aspect >= 1.0 {
        egui::vec2(SIZE, SIZE / thumbnail.content_aspect)
    } else {
        egui::vec2(SIZE * thumbnail.content_aspect, SIZE)
    };
    let content = egui::Rect::from_center_size(slot.center(), content_size);
    egui::Image::new((thumbnail.texture.id(), content_size))
        .uv(thumbnail.content_uv)
        .corner_radius(12)
        .alt_text("Open artwork")
        .paint_at(ui, content);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
