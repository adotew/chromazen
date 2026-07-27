use std::path::PathBuf;

use crate::artwork::{ArtworkId, ArtworkSummary};

use super::super::command::AppCommand;

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
            ui.add_space(20.0);
            let (header_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 32.0),
                egui::Sense::hover(),
            );
            ui.painter().text(
                header_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Chromazen",
                egui::TextStyle::Heading.resolve(ui.style()),
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
                commands.push(AppCommand::NewArtwork);
            }
            if let Some(warning) = warning {
                ui.add_space(8.0);
                ui.colored_label(egui::Color32::LIGHT_RED, warning);
            }
            ui.add_space(18.0);

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
                    ui.spacing_mut().item_spacing = egui::vec2(18.0, 18.0);
                    for artwork in artworks {
                        ui.allocate_ui_with_layout(
                            egui::vec2(220.0, 250.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::Frame::NONE.inner_margin(0).show(ui, |ui| {
                                    ui.set_width(198.0);
                                    let texture = self.texture(&artwork.id);
                                    let image = texture.map_or_else(
                                        || {
                                            egui::Image::new(egui::include_image!(
                                                "../../../assets/icons/paintbrush.svg"
                                            ))
                                            .tint(ui.visuals().weak_text_color())
                                        },
                                        |texture| {
                                            egui::Image::new((texture, egui::vec2(198.0, 198.0)))
                                        },
                                    );
                                    let open = ui.add(
                                        egui::Button::image(
                                            image
                                                .corner_radius(12)
                                                .fit_to_exact_size(egui::vec2(198.0, 198.0)),
                                        )
                                        .frame(false),
                                    );
                                    if open.clicked() {
                                        commands.push(AppCommand::OpenArtwork(artwork.id.clone()));
                                    }
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.strong(&artwork.title);
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let menu_icon = egui::Image::new(
                                                    egui::include_image!(
                                                        "../../../assets/icons/ellipsis-vertical.svg"
                                                    ),
                                                )
                                                .fit_to_exact_size(egui::Vec2::splat(18.0))
                                                .alt_text("Artwork actions");
                                                let menu_button = ui
                                                    .add(
                                                        egui::Button::image(menu_icon)
                                                            .image_tint_follows_text_color(true)
                                                            .frame(false)
                                                            .min_size(egui::vec2(28.0, 24.0)),
                                                    )
                                                    .on_hover_text("Artwork actions");
                                                egui::Popup::menu(&menu_button).show(|ui| {
                                                    if ui.button("Rename").clicked() {
                                                        self.rename = Some((
                                                            artwork.id.clone(),
                                                            artwork.title.clone(),
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
                                            },
                                        );
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

    fn texture(&self, id: &ArtworkId) -> Option<egui::TextureId> {
        self.thumbnails
            .iter()
            .find(|thumbnail| &thumbnail.id == id)
            .map(|thumbnail| thumbnail.texture.id())
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
                        commands.push(AppCommand::RenameArtwork {
                            id: id.clone(),
                            title: title.clone(),
                        });
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
                        commands.push(AppCommand::DeleteArtwork(id.clone()));
                        close = true;
                    }
                });
            });
        if close {
            self.delete = None;
        }
    }
}
