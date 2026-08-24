use super::*;

impl GuiLayer {
    pub(super) fn sync_reference_textures(&mut self, references: &[ReferenceImage]) {
        self.reference_textures.retain(|cached| {
            references.iter().any(|reference| {
                reference.id == cached.id && reference.resource_version == cached.resource_version
            })
        });
        for reference in references {
            if self.reference_texture(reference.id).is_some() {
                continue;
            }
            let size = [
                reference.pixels.width() as usize,
                reference.pixels.height() as usize,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, reference.pixels.as_raw());
            let texture = self.context.load_texture(
                format!("reference {}", reference.id.0),
                image,
                egui::TextureOptions::LINEAR,
            );
            self.reference_textures.push(ReferenceTexture {
                id: reference.id,
                resource_version: reference.resource_version,
                texture,
            });
        }
        if self
            .selected_reference
            .is_some_and(|id| references.iter().all(|reference| reference.id != id))
        {
            self.selected_reference = None;
        }
    }

    pub(super) fn reference_texture(&self, id: ReferenceId) -> Option<egui::TextureId> {
        self.reference_textures
            .iter()
            .find(|cached| cached.id == id)
            .map(|cached| cached.texture.id())
    }

    pub(super) fn show_workspace_references(
        &mut self,
        context: &egui::Context,
        references: &[ReferenceImage],
        view: PaintViewSnapshot,
        workspace_rect: egui::Rect,
    ) {
        self.reference_hit_rects.clear();
        self.pointer_over_reference = false;
        self.pointer_over_selected_reference = false;
        let pointer_position = context.pointer_latest_pos();
        let pixels_per_point = context.pixels_per_point();
        for reference in references.iter().filter(|reference| reference.visible) {
            let Some(texture_id) = self.reference_texture(reference.id) else {
                continue;
            };
            let window_position = view.workspace_to_window(reference.position);
            let position = egui::pos2(
                window_position[0] / pixels_per_point,
                window_position[1] / pixels_per_point,
            );
            let size = egui::vec2(
                reference.size[0] * view.zoom / pixels_per_point,
                reference.size[1] * view.zoom / pixels_per_point,
            );
            if size.x <= 0.5 || size.y <= 0.5 {
                continue;
            }

            let rect = egui::Rect::from_min_size(position, size);
            let pointer_over_image =
                pointer_over_visible_reference(pointer_position, rect, workspace_rect);
            let pointer_over_resize = !reference.locked
                && self.selected_reference == Some(reference.id)
                && pointer_position.is_some_and(|pointer| {
                    workspace_rect.contains(pointer)
                        && reference_resize_handle(rect).1.contains(pointer)
                });
            self.pointer_over_reference |= pointer_over_image || pointer_over_resize;
            if self.selected_reference == Some(reference.id) {
                self.pointer_over_selected_reference = pointer_over_image || pointer_over_resize;
            }
            let visible_rect = rect.intersect(workspace_rect);
            if !visible_rect.is_positive() {
                continue;
            }
            self.reference_hit_rects.push(visible_rect);
            if !reference.locked && self.selected_reference == Some(reference.id) {
                let resize_rect = reference_resize_handle(rect).1.intersect(workspace_rect);
                if resize_rect.is_positive() {
                    self.reference_hit_rects.push(resize_rect);
                }
            }

            let area = egui::Area::new(egui::Id::new(("reference", reference.id.0)))
                .order(egui::Order::Middle)
                .fixed_pos(visible_rect.min)
                .default_size(visible_rect.size())
                // Keep the canvas-relative transform authoritative. The area's interactive bounds
                // are only the visible image, while its painter clips the full image to the
                // workspace instead of moving the reference back inside the window.
                .constrain(false)
                .show(context, |ui| {
                    ui.shrink_clip_rect(workspace_rect);
                    let sense = if reference.locked {
                        egui::Sense::click()
                    } else {
                        egui::Sense::click_and_drag()
                    };
                    let response = ui.allocate_rect(visible_rect, sense);
                    ui.painter().image(
                        texture_id,
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    let resize = (!reference.locked
                        && self.selected_reference == Some(reference.id))
                    .then(|| {
                        let (_, handle_rect) = reference_resize_handle(rect);
                        ui.interact(
                            handle_rect.intersect(workspace_rect),
                            egui::Id::new(("reference resize", reference.id.0)),
                            egui::Sense::drag(),
                        )
                        .on_hover_and_drag_cursor(egui::CursorIcon::ResizeNwSe)
                    });
                    (response, resize)
                });
            let (response, resize) = area.inner;
            if response.clicked()
                || response.secondary_clicked()
                || resize.as_ref().is_some_and(egui::Response::drag_started)
            {
                self.selected_reference = Some(reference.id);
            }
            self.show_reference_context_menu(&response, reference);

            if !reference.locked {
                let active = if let Some(resize) = resize.as_ref().filter(|resize| resize.dragged())
                {
                    resize
                        .total_drag_delta()
                        .map(|delta| (ReferenceDrag::Resize, delta))
                } else if response.dragged() {
                    response
                        .total_drag_delta()
                        .map(|delta| (ReferenceDrag::Move, delta))
                } else {
                    None
                };
                let resize_started = resize.as_ref().is_some_and(egui::Response::drag_started);
                let started_drag = if resize_started {
                    Some(ReferenceDrag::Resize)
                } else if response.drag_started() {
                    Some(ReferenceDrag::Move)
                } else {
                    None
                };
                if let Some(drag) = started_drag {
                    self.reference_transform_edit = Some(ReferenceTransformEdit {
                        id: reference.id,
                        drag,
                        position: reference.position,
                        size: reference.size,
                    });
                }
                if let Some((drag, delta)) = active {
                    self.update_reference_drag(reference.id, drag, delta, view, pixels_per_point);
                }
                let resize_stopped = resize.as_ref().is_some_and(egui::Response::drag_stopped);
                if response.drag_stopped() || resize_stopped {
                    self.commit_reference_drag(reference.id);
                }
            }

            if self.selected_reference == Some(reference.id) {
                let painter = context
                    .layer_painter(area.response.layer_id)
                    .with_clip_rect(workspace_rect);
                paint_reference_selection(&painter, rect, !reference.locked);
            }
        }

        if !context.input(|input| input.pointer.primary_down())
            && let Some(id) = self.reference_transform_edit.as_ref().map(|edit| edit.id)
        {
            self.commit_reference_drag(id);
        }
    }

    pub(super) fn show_reference_context_menu(
        &mut self,
        response: &egui::Response,
        reference: &ReferenceImage,
    ) {
        response.context_menu(|ui| {
            ui.set_min_width(80.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            if ui
                .button(if reference.locked { "Unlock" } else { "Lock" })
                .clicked()
            {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::ToggleReferenceLocked(
                        reference.id,
                    )));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                self.commands
                    .push(AppCommand::Editor(EditorCommand::DeleteReference(
                        reference.id,
                    )));
                ui.close();
            }
        });
    }

    pub(crate) fn pointer_over_reference(&self) -> bool {
        self.pointer_over_reference
    }

    pub(crate) fn reference_drag_active(&self) -> bool {
        self.reference_transform_edit.is_some()
    }

    pub(crate) fn reference_resize_active(&self) -> bool {
        self.reference_transform_edit
            .is_some_and(|edit| edit.drag == ReferenceDrag::Resize)
    }

    pub(crate) fn window_point_over_reference(&self, point: [f32; 2]) -> bool {
        window_point_over_rects(
            point,
            self.context.pixels_per_point(),
            &self.reference_hit_rects,
        )
    }

    pub(super) fn clear_reference_selection_on_outside_press(&mut self, context: &egui::Context) {
        let primary_pressed = context.input(|input| input.pointer.primary_pressed());
        if should_clear_reference_selection(
            self.selected_reference.is_some(),
            primary_pressed,
            self.pointer_over_selected_reference,
            context.is_pointer_over_egui(),
        ) {
            self.selected_reference = None;
        }
    }

    pub(super) fn update_reference_drag(
        &mut self,
        id: ReferenceId,
        drag: ReferenceDrag,
        drag_delta: egui::Vec2,
        view: PaintViewSnapshot,
        pixels_per_point: f32,
    ) {
        let Some(origin) = self.reference_transform_edit.filter(|edit| edit.id == id) else {
            return;
        };
        let delta = view.window_delta_to_workspace([
            drag_delta.x * pixels_per_point,
            drag_delta.y * pixels_per_point,
        ]);
        let (position, size) =
            reference_transform_from_drag_origin(origin.position, origin.size, drag, delta);
        self.commands
            .push(AppCommand::Editor(EditorCommand::SetReferenceTransform {
                id,
                position,
                size,
            }));
    }

    pub(super) fn commit_reference_drag(&mut self, id: ReferenceId) {
        if self
            .reference_transform_edit
            .is_some_and(|edit| edit.id == id)
        {
            self.reference_transform_edit = None;
        }
    }
}
