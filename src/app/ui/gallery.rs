use std::path::PathBuf;

use egui::containers::menu::MenuButton;

use crate::artwork::{ArtworkId, ArtworkSummary};

use super::super::{
    command::{AppCommand, GalleryCommand, NavigationCommand},
    gallery::ThumbnailCompletion,
};
use super::*;

pub(super) const ARTWORK_RAIL_WIDTH: f32 = 264.0;
const ARTWORK_ROW_HEIGHT: f32 = 72.0;
const ARTWORK_TEXT_GAP: f32 = 5.0;
const THUMBNAIL_SIZE: f32 = 53.0;
const ARTWORK_TITLE_SIZE: f32 = 17.0;
const ARTWORK_DIMENSIONS_SIZE: f32 = 13.0;
const RAIL_ICON_SIZE: f32 = 22.0;
const RAIL_BUTTON_SIZE: f32 = 38.0;

#[derive(Default)]
pub(super) struct GalleryUi {
    thumbnails: Vec<Thumbnail>,
    rename: Option<(ArtworkId, String)>,
    delete: Option<(ArtworkId, String)>,
    collapsed: bool,
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
        active: Option<(&ArtworkId, &str, [u32; 2])>,
        warning: Option<&str>,
        commands: &mut Vec<AppCommand>,
    ) -> f32 {
        self.sync_thumbnails(artworks);
        let rail_progress = ui.ctx().animate_bool_with_time_and_easing(
            egui::Id::new("artwork sidebar animation"),
            !self.collapsed,
            0.18,
            egui::emath::easing::cubic_in_out,
        );
        if rail_progress > 0.0 {
            self.show_rail(ui, artworks, active, warning, commands, rail_progress);
        }
        self.show_rename_dialog(ui.ctx(), commands);
        self.show_delete_dialog(ui.ctx(), commands);
        rail_progress
    }

    pub(super) fn toggle_visible(&mut self) {
        self.collapsed = !self.collapsed;
    }

    fn show_rail(
        &mut self,
        ui: &mut egui::Ui,
        artworks: &[ArtworkSummary],
        active: Option<(&ArtworkId, &str, [u32; 2])>,
        warning: Option<&str>,
        commands: &mut Vec<AppCommand>,
        rail_progress: f32,
    ) {
        let rail_fill = ui.visuals().window_fill();
        let rail_frame = egui::Frame::side_top_panel(ui.style())
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE);
        egui::Panel::left("artwork tabs")
            .frame(rail_frame)
            .exact_size(ARTWORK_RAIL_WIDTH * rail_progress)
            .resizable(false)
            .show_separator_line(false)
            .show_inside(ui, |panel_ui| {
                let layer_id =
                    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("artwork sidebar"));
                panel_ui.ctx().move_to_top(layer_id);
                let mut panel_ui = panel_ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("foreground")
                        .layer_id(layer_id),
                );
                let component_rect = egui::Frame::side_top_panel(panel_ui.style())
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .widget_rect(panel_ui.max_rect());
                paint_rounded_panel(&panel_ui, component_rect, egui::CornerRadius::ZERO);
                let inner_width = ARTWORK_RAIL_WIDTH
                    - egui::Frame::side_top_panel(panel_ui.style())
                        .inner_margin
                        .sum()
                        .x;
                let content_rect = egui::Rect::from_min_size(
                    egui::pos2(
                        panel_ui.max_rect().right() - inner_width,
                        panel_ui.min_rect().top(),
                    ),
                    egui::vec2(inner_width, panel_ui.available_height()),
                );
                let mut ui = panel_ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt("sidebar contents")
                        .max_rect(content_rect),
                );
                ui.set_clip_rect(panel_ui.clip_rect());
                ui.style_mut().visuals.panel_fill = rail_fill;
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Chromazen").font(egui::FontId::new(
                        24.0,
                        egui::FontFamily::Name("elms_sans_light".into()),
                    )));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let add_icon = egui::Image::new(egui::include_image!(
                            "../../../assets/icons/plus.svg"
                        ))
                        .fit_to_exact_size(egui::Vec2::splat(RAIL_ICON_SIZE))
                        .alt_text("New artwork");
                        if ui
                            .add(
                                egui::Button::image(add_icon)
                                    .frame_when_inactive(false)
                                    .image_tint_follows_text_color(true)
                                    .corner_radius(10)
                                    .min_size(egui::Vec2::splat(RAIL_BUTTON_SIZE)),
                            )
                            .on_hover_text("New artwork")
                            .clicked()
                        {
                            commands.push(AppCommand::Navigation(NavigationCommand::NewArtwork));
                        }
                    });
                });
                ui.add_space(14.0);
                if let Some(warning) = warning {
                    ui.colored_label(egui::Color32::LIGHT_RED, warning);
                    ui.add_space(10.0);
                }

                egui::Panel::bottom("artwork rail footer")
                    .show_separator_line(false)
                    .show_inside(&mut ui, |ui| {
                        ui.add_space(5.0);
                        let icon = egui::Image::new(egui::include_image!(
                            "../../../assets/icons/panel-left.svg"
                        ))
                        .fit_to_exact_size(egui::Vec2::splat(RAIL_ICON_SIZE))
                        .alt_text("Hide artwork tabs");
                        if ui
                            .add(
                                egui::Button::image(icon)
                                    .frame_when_inactive(false)
                                    .image_tint_follows_text_color(true)
                                    .corner_radius(10)
                                    .min_size(egui::Vec2::splat(RAIL_BUTTON_SIZE)),
                            )
                            .on_hover_text(format!("Hide artwork tabs ({RAIL_SHORTCUT})"))
                            .clicked()
                        {
                            self.collapsed = true;
                        }
                        ui.add_space(5.0);
                    });

                ui.spacing_mut().item_spacing.y = 5.0;
                let pending_active =
                    active.filter(|(id, _, _)| artworks.iter().all(|artwork| artwork.id != **id));
                let pending_count = usize::from(pending_active.is_some());
                egui::ScrollArea::vertical().show_rows(
                    &mut ui,
                    ARTWORK_ROW_HEIGHT,
                    artworks.len() + pending_count,
                    |ui, rows| {
                        for row in rows {
                            if row == 0
                                && let Some((id, title, dimensions)) = pending_active
                            {
                                self.show_artwork_row(ui, id, title, dimensions, true, commands);
                                continue;
                            }
                            let artwork = &artworks[row - pending_count];
                            let selected = active.is_some_and(|(id, _, _)| id == &artwork.id);
                            let (title, dimensions) = if selected {
                                active
                                    .map(|(_, title, dimensions)| (title, dimensions))
                                    .unwrap_or((&artwork.title, artwork.dimensions))
                            } else {
                                (artwork.title.as_str(), artwork.dimensions)
                            };
                            self.show_artwork_row(
                                ui,
                                &artwork.id,
                                title,
                                dimensions,
                                selected,
                                commands,
                            );
                        }
                    },
                );
            });
    }

    fn show_artwork_row(
        &mut self,
        ui: &mut egui::Ui,
        id: &ArtworkId,
        title: &str,
        dimensions: [u32; 2],
        selected: bool,
        commands: &mut Vec<AppCommand>,
    ) {
        let (rect, response) =
            selectable_row(ui, ARTWORK_ROW_HEIGHT, egui::Sense::click(), selected);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, title)
        });

        let thumbnail_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 10.0 + THUMBNAIL_SIZE / 2.0, rect.center().y),
            egui::Vec2::splat(THUMBNAIL_SIZE),
        );
        paint_artwork_thumbnail(ui, thumbnail_rect, self.thumbnail(id));

        let menu_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 23.0, rect.center().y),
            egui::Vec2::splat(RAIL_BUTTON_SIZE),
        );
        let text_height = ARTWORK_TITLE_SIZE + ARTWORK_TEXT_GAP + ARTWORK_DIMENSIONS_SIZE;
        let title_rect = egui::Rect::from_min_max(
            egui::pos2(
                thumbnail_rect.right() + 12.0,
                rect.center().y - text_height / 2.0,
            ),
            egui::pos2(menu_rect.left() - 5.0, rect.center().y + text_height / 2.0),
        );
        let mut title_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("artwork title", id.as_str()))
                .max_rect(title_rect)
                .layout(egui::Layout::top_down(egui::Align::LEFT)),
        );
        title_ui.spacing_mut().item_spacing.y = ARTWORK_TEXT_GAP;
        title_ui.add(
            egui::Label::new(egui::RichText::new(title).strong().size(ARTWORK_TITLE_SIZE))
                .truncate()
                .selectable(false),
        );
        title_ui.add(
            egui::Label::new(
                egui::RichText::new(format_dimensions(dimensions))
                    .weak()
                    .size(ARTWORK_DIMENSIONS_SIZE),
            )
            .truncate()
            .selectable(false),
        );

        let mut menu_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("artwork menu", id.as_str()))
                .max_rect(menu_rect),
        );
        let icon = egui::Image::new(egui::include_image!(
            "../../../assets/icons/ellipsis-vertical.svg"
        ))
        .fit_to_exact_size(egui::Vec2::splat(RAIL_ICON_SIZE))
        .alt_text("Artwork menu");
        let (menu_response, _) = MenuButton::from_button(
            egui::Button::image(icon)
                .frame_when_inactive(false)
                .corner_radius(10)
                .min_size(egui::Vec2::splat(RAIL_BUTTON_SIZE)),
        )
        .ui(&mut menu_ui, |ui| {
            self.show_artwork_menu(ui, id, title, commands);
        });
        menu_response.on_hover_text("Artwork menu");

        response.context_menu(|ui| self.show_artwork_menu(ui, id, title, commands));
        if response.clicked() {
            commands.push(AppCommand::Navigation(NavigationCommand::OpenArtwork(
                id.clone(),
            )));
        }
    }

    #[cfg(test)]
    fn is_collapsed(&self) -> bool {
        self.collapsed
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
        id: &ArtworkId,
        title: &str,
        commands: &mut Vec<AppCommand>,
    ) {
        apply_menu_item_padding(ui);
        if ui.button("Rename").clicked() {
            self.rename = Some((id.clone(), title.to_owned()));
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            commands.push(AppCommand::Gallery(GalleryCommand::Duplicate(id.clone())));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.delete = Some((id.clone(), title.to_owned()));
            ui.close();
        }
    }
}

fn format_dimensions(dimensions: [u32; 2]) -> String {
    format!("{} × {}", dimensions[0], dimensions[1])
}

fn paint_artwork_thumbnail(ui: &egui::Ui, slot: egui::Rect, thumbnail: Option<&Thumbnail>) {
    let Some(thumbnail) = thumbnail else {
        egui::Image::new(egui::include_image!("../../../assets/icons/paintbrush.svg"))
            .tint(ui.visuals().weak_text_color())
            .corner_radius(8)
            .fit_to_exact_size(slot.size())
            .paint_at(ui, slot);
        return;
    };

    let content_size = if thumbnail.content_aspect >= 1.0 {
        egui::vec2(THUMBNAIL_SIZE, THUMBNAIL_SIZE / thumbnail.content_aspect)
    } else {
        egui::vec2(THUMBNAIL_SIZE * thumbnail.content_aspect, THUMBNAIL_SIZE)
    };
    let content = egui::Rect::from_center_size(slot.center(), content_size);
    egui::Image::new((thumbnail.texture.id(), content_size))
        .uv(thumbnail.content_uv)
        .corner_radius(8)
        .alt_text("Open artwork")
        .paint_at(ui, content);
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
    fn the_rail_starts_visible_and_toggles_off() {
        let mut gallery = GalleryUi::default();
        assert!(!gallery.is_collapsed());

        gallery.toggle_visible();
        assert!(gallery.is_collapsed());

        gallery.toggle_visible();
        assert!(!gallery.is_collapsed());
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
}
