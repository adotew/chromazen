use std::{
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.heading("Your artwork");
                ui.add_space(12.0);
                if ui.button("New Artwork").clicked() {
                    commands.push(AppCommand::NewArtwork);
                }
            });
            if let Some(warning) = warning {
                ui.add_space(8.0);
                ui.colored_label(egui::Color32::LIGHT_RED, warning);
            }
            ui.add_space(18.0);

            if artworks.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("No artwork yet");
                    ui.label("Create an artwork to begin painting.");
                    ui.add_space(12.0);
                    if ui.button("Create Artwork").clicked() {
                        commands.push(AppCommand::NewArtwork);
                    }
                });
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(18.0, 18.0);
                    for artwork in artworks {
                        ui.allocate_ui(egui::vec2(220.0, 278.0), |ui| {
                            egui::Frame::group(ui.style())
                                .corner_radius(12)
                                .inner_margin(10)
                                .show(ui, |ui| {
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
                                            image.fit_to_exact_size(egui::vec2(198.0, 198.0)),
                                        )
                                        .frame(false),
                                    );
                                    if open.clicked() {
                                        commands.push(AppCommand::OpenArtwork(artwork.id.clone()));
                                    }
                                    ui.add_space(6.0);
                                    ui.strong(&artwork.title);
                                    ui.small(modified_label(artwork.modified_unix_ms));
                                    ui.horizontal(|ui| {
                                        if ui.small_button("Rename").clicked() {
                                            self.rename =
                                                Some((artwork.id.clone(), artwork.title.clone()));
                                        }
                                        if ui.small_button("Delete").clicked() {
                                            self.delete =
                                                Some((artwork.id.clone(), artwork.title.clone()));
                                        }
                                    });
                                });
                        });
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

fn modified_label(modified_unix_ms: u64) -> String {
    let modified = UNIX_EPOCH + Duration::from_millis(modified_unix_ms);
    let elapsed = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        "Modified just now".to_owned()
    } else if seconds < 3600 {
        format!("Modified {}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("Modified {}h ago", seconds / 3600)
    } else {
        format!("Modified {}d ago", seconds / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_modified_times_are_human_readable() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert_eq!(modified_label(now), "Modified just now");
    }
}
