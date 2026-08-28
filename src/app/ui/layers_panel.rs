use super::*;

impl GuiLayer {
    pub(super) fn show_layers_panel(
        &mut self,
        ui: &mut egui::Ui,
        layers: &LayerSnapshot,
        background: egui::Color32,
    ) {
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("layer list")
            .max_height(LAYER_LIST_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for layer in layers.layers.iter().rev() {
                    let selected = layers.selection == layer.id;
                    let thumbnail = self
                        .layer_thumbnails
                        .iter()
                        .find(|thumbnail| thumbnail.key.layer_id == layer.id)
                        .map(|thumbnail| thumbnail.texture_id);
                    let renaming = self
                        .layer_name_edit
                        .as_ref()
                        .is_some_and(|edit| edit.id == layer.id);
                    let clipped_name = layer.clipped.then(|| format!("↳ {}", layer.name));
                    let row = show_layer_row(
                        ui,
                        LayerRow {
                            name: if renaming {
                                ""
                            } else {
                                clipped_name.as_deref().unwrap_or(&layer.name)
                            },
                            selected,
                            texture_id: thumbnail,
                            solid_color: None,
                            visible: Some(layer.visible),
                            opacity: Some(layer.opacity),
                            drag_id: Some(layer.id),
                        },
                    );
                    if let (Some(pointer), Some(dragged)) = (
                        ui.input(|input| input.pointer.interact_pos()),
                        row.row.dnd_hover_payload::<LayerId>(),
                    ) && *dragged != layer.id
                    {
                        let edge = if pointer.y < row.row.rect.center().y {
                            DropEdge::Above
                        } else {
                            DropEdge::Below
                        };
                        let y = match edge {
                            DropEdge::Above => row.row.rect.top(),
                            DropEdge::Below => row.row.rect.bottom(),
                        };
                        ui.painter().hline(
                            row.row.rect.x_range().shrink(8.0),
                            y,
                            egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
                        );
                        if let Some(dragged) = row.row.dnd_release_payload::<LayerId>() {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::MoveLayer {
                                    dragged: *dragged,
                                    target: layer.id,
                                    edge,
                                }));
                        }
                    }
                    if row.mode.as_ref().is_some_and(egui::Response::clicked) {
                        self.layer_opacity_open = if self.layer_opacity_open == Some(layer.id) {
                            None
                        } else {
                            Some(layer.id)
                        };
                        if !selected {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::SelectLayer(layer.id)));
                        }
                    } else if row.visibility.as_ref().is_some_and(egui::Response::clicked) {
                        self.commands
                            .push(AppCommand::Editor(EditorCommand::SetLayerVisibility {
                                id: layer.id,
                                visible: !layer.visible,
                            }));
                    } else if (row.row.clicked() || row.row.secondary_clicked()) && !selected {
                        self.commands
                            .push(AppCommand::Editor(EditorCommand::SelectLayer(layer.id)));
                    }

                    let mut start_rename = false;
                    let layer_index = layers
                        .layers
                        .iter()
                        .position(|candidate| candidate.id == layer.id)
                        .expect("displayed layer must exist in snapshot");
                    let can_merge_down = layer.visible
                        && merge_down_target_index(layer_index).is_some_and(|lower_index| {
                            layers.layers[lower_index].visible
                                && !layers.layers[lower_index].clipped
                        })
                        && !layers
                            .layers
                            .get(layer_index + 1)
                            .is_some_and(|upper| upper.clipped);
                    let can_delete =
                        layers.layers.len() == 1 || (layer_index != 0 || !layers.layers[1].clipped);
                    row.row.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            start_rename = true;
                            ui.close();
                        }
                        if ui.button("Duplicate").clicked() {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::DuplicateSelectedLayer));
                            ui.close();
                        }
                        if ui
                            .add_enabled(can_merge_down, egui::Button::new("Merge Down"))
                            .clicked()
                        {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::MergeLayerDown(layer.id)));
                            ui.close();
                        }
                        let clipping_label = if layer.clipped {
                            "Release Clipping Mask"
                        } else {
                            "Clip to Layer Below"
                        };
                        if ui
                            .add_enabled(
                                layer.clipped || layer_index > 0,
                                egui::Button::new(clipping_label),
                            )
                            .clicked()
                        {
                            self.commands.push(AppCommand::Editor(
                                EditorCommand::SetLayerClipped {
                                    id: layer.id,
                                    clipped: !layer.clipped,
                                },
                            ));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Clear Layer").clicked() {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::ClearLayer));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                can_delete,
                                egui::Button::new(
                                    egui::RichText::new("Delete").color(egui::Color32::LIGHT_RED),
                                ),
                            )
                            .clicked()
                        {
                            self.commands
                                .push(AppCommand::Editor(EditorCommand::DeleteSelectedLayer));
                            ui.close();
                        }
                    });
                    if start_rename {
                        self.layer_name_edit = Some(LayerNameEdit {
                            id: layer.id,
                            name: layer.name.clone(),
                        });
                    }

                    if renaming {
                        let mut edited_name = self
                            .layer_name_edit
                            .as_ref()
                            .map_or_else(|| layer.name.clone(), |edit| edit.name.clone());
                        let response = ui.put(
                            row.name_rect,
                            egui::TextEdit::singleline(&mut edited_name)
                                .desired_width(row.name_rect.width()),
                        );
                        if !response.has_focus() && !response.lost_focus() {
                            response.request_focus();
                        }
                        let cancel = response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Escape));
                        let commit = !cancel
                            && (response.lost_focus()
                                || (response.has_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter))));
                        if cancel {
                            self.layer_name_edit = None;
                        } else if commit {
                            if !edited_name.trim().is_empty() {
                                self.commands.push(AppCommand::Editor(
                                    EditorCommand::RenameLayer {
                                        id: layer.id,
                                        name: edited_name,
                                    },
                                ));
                            }
                            self.layer_name_edit = None;
                        } else if let Some(edit) = self.layer_name_edit.as_mut() {
                            edit.name = edited_name;
                        }
                    }

                    if self.layer_opacity_open == Some(layer.id) {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: 14,
                                right: 14,
                                top: 6,
                                bottom: 0,
                            })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Opacity");
                                    let mut opacity = layer.opacity;
                                    let opacity_changed = ui
                                        .add(
                                            egui::Slider::new(&mut opacity, 0..=100)
                                                .suffix("%")
                                                .show_value(true),
                                        )
                                        .on_hover_text("Layer opacity")
                                        .changed();
                                    if opacity_changed {
                                        let edit = self.layer_opacity_edit.get_or_insert(
                                            LayerOpacityEdit {
                                                id: layer.id,
                                                before: layer.opacity,
                                                current: opacity,
                                            },
                                        );
                                        if edit.id != layer.id {
                                            *edit = LayerOpacityEdit {
                                                id: layer.id,
                                                before: layer.opacity,
                                                current: opacity,
                                            };
                                        } else {
                                            edit.current = opacity;
                                        }
                                        self.commands.push(AppCommand::Editor(
                                            EditorCommand::SetLayerOpacity {
                                                id: layer.id,
                                                opacity,
                                            },
                                        ));
                                    }
                                });
                            });
                    }

                    ui.add_space(4.0);
                }

                if !ui.ctx().input(|input| input.pointer.primary_down())
                    && let Some(edit) = self.layer_opacity_edit.take()
                {
                    self.commands
                        .push(AppCommand::Editor(EditorCommand::CommitLayerOpacity {
                            id: edit.id,
                            before: edit.before,
                            after: edit.current,
                        }));
                }

                let mut color = background;
                let response = show_layer_row(
                    ui,
                    LayerRow {
                        name: "Background",
                        selected: false,
                        texture_id: None,
                        solid_color: Some(background),
                        visible: None,
                        opacity: None,
                        drag_id: None,
                    },
                );
                egui::Popup::from_toggle_button_response(&response.row)
                    .width(220.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if color_picker::show(ui, &mut color) {
                            self.background_edit_start.get_or_insert(rgb(background));
                            self.commands.push(AppCommand::Editor(
                                EditorCommand::SetBackgroundColor(rgb(color)),
                            ));
                        }
                    });
                if !ui.ctx().input(|input| input.pointer.primary_down())
                    && let Some(before) = self.background_edit_start.take()
                {
                    self.commands
                        .push(AppCommand::Editor(EditorCommand::CommitBackgroundColor {
                            before,
                            after: rgb(color),
                        }));
                }
            });
    }
}
