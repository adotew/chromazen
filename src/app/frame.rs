use super::*;

impl App {
    pub(super) fn render(&mut self, window: &Window, event_loop: &ActiveEventLoop) {
        let mut app_action_processed = self.process_export_completion();
        app_action_processed |= self.process_gallery_completion();
        app_action_processed |= self.process_brush_import_completion();
        app_action_processed |= self.process_reference_import_completions();
        app_action_processed |= self.process_reference_load_completions();
        app_action_processed |= self.process_pending_commands();
        let mut brush_switched = self.apply_pending_brush_change();

        if self.pending_exit && self.screen == AppScreen::Gallery && !self.export.is_exporting() {
            event_loop.exit();
            return;
        }
        if self.screen == AppScreen::Editor
            && let Some(paint) = self.paint.as_ref()
        {
            app_action_processed |= self.autosave.update(paint, &self.references);
            if self.pending_exit
                && !self.reference_load.is_loading()
                && self.autosave.is_clean(paint, &self.references)
                && !self.export.is_exporting()
            {
                event_loop.exit();
                return;
            }
            if self.pending_gallery
                && !self.reference_load.is_loading()
                && self.autosave.is_clean(paint, &self.references)
            {
                let new_size = self.pending_new_artwork;
                self.finish_gallery_navigation();
                if let Some(size) = new_size {
                    self.create_artwork(size);
                }
                app_action_processed = true;
            }
        }

        if self
            .paint
            .as_ref()
            .is_none_or(|paint| paint.surface_size()[0] == 0 || paint.surface_size()[1] == 0)
        {
            return;
        }
        let layer_content_bounds = (self.screen == AppScreen::Editor
            && self.input.tool().paint_tool().is_none())
        .then(|| {
            self.paint
                .as_mut()
                .and_then(PaintRenderer::selected_layer_content_bounds)
        })
        .flatten();
        let Some(paint) = self.paint.as_ref() else {
            return;
        };

        let (full_output, commands) = {
            let Some(gui) = self.gui.as_mut() else {
                return;
            };
            let output = match self.screen {
                AppScreen::Gallery => {
                    let warning = self.gallery.warning();
                    gui.run_gallery(window, self.gallery.artworks(), warning.as_deref())
                }
                AppScreen::Editor => {
                    gui.sync_layer_thumbnails(paint);
                    let layer_snapshot = paint.layer_snapshot();
                    let brush_resize_label =
                        self.input
                            .brush_resize_pos()
                            .map(|center| BrushResizeLabel {
                                center,
                                outline_half_width: paint.brush_outline_half_size(gui.brush.size)
                                    [0],
                            });
                    let eyedropper_indicator =
                        self.input
                            .eyedropper_indicator_pos()
                            .map(|center| EyedropperIndicator {
                                center,
                                color: gui.brush.color,
                            });
                    let status = self.autosave.status(paint, &self.references);
                    let pending_navigation = if self.reference_load.is_loading() {
                        None
                    } else if self.pending_exit {
                        Some("Closing Chromazen")
                    } else if self.pending_new_artwork.is_some() {
                        Some("Creating New Artwork")
                    } else if self.pending_gallery {
                        Some("Returning to Gallery")
                    } else {
                        None
                    };
                    gui.run_editor(
                        window,
                        EditorUiState {
                            layers: &layer_snapshot,
                            tool: self.input.tool(),
                            layer_transform: paint.active_layer_transform(),
                            layer_content_bounds,
                            brush_resize_label,
                            eyedropper_indicator,
                            save_status: status,
                            pending_navigation,
                            brush_import_dialog_delay: self.brush_import.dialog_delay(),
                            reference_import_dialog_delay: self.reference_import.dialog_delay(),
                            reference_load_dialog_delay: self.reference_load.dialog_delay(),
                            references: self.references.images(),
                            workspace_view: paint.view_snapshot(),
                        },
                    )
                }
            };
            (output, gui.take_commands())
        };
        self.pending_commands.extend(commands);
        app_action_processed |= self.process_pending_commands();

        if self.screen == AppScreen::Editor
            && let Some(paint) = self.paint.as_ref()
        {
            app_action_processed |= self.autosave.update(paint, &self.references);
        }
        let Some(outcome) = self.render_frame(window, full_output) else {
            return;
        };
        brush_switched |= self.apply_pending_brush_change();
        self.update_repaint_schedule(
            outcome.repaint_delay,
            window,
            outcome.canvas_needs_redraw || app_action_processed || brush_switched,
        );
    }

    pub(super) fn render_frame(
        &mut self,
        window: &Window,
        full_output: egui::FullOutput,
    ) -> Option<RenderOutcome> {
        let cursor_pos = self.input.brush_cursor_pos();
        let brush_resize_pos = self.input.brush_resize_pos();
        let resize_is_anchored = self.input.brush_resize_is_anchored();
        let is_resizing_brush = self.input.is_resizing_brush();
        let is_panning = self.input.is_panning();
        let is_rotating_canvas = self.input.is_rotating_canvas();
        let is_pan_modifier_active = self.input.is_pan_modifier_active();
        let is_eyedropper_active = self.input.is_eyedropper_active();
        let brush_pressure = self.pressure_state.brush_pressure();
        let paint = self.paint.as_mut()?;
        let gui = self.gui.as_mut()?;
        let pointer_over_ui = gui.context.is_pointer_over_egui();
        let pointer_over_reference =
            self.screen == AppScreen::Editor && gui.pointer_over_reference();
        let reference_drag_active = self.screen == AppScreen::Editor && gui.reference_drag_active();
        let reference_resize_active =
            self.screen == AppScreen::Editor && gui.reference_resize_active();
        let pointer_over_ui_or_reference =
            pointer_over_ui || pointer_over_reference || reference_drag_active;
        let brush_cursor = brush_resize_pos
            .filter(|_| resize_is_anchored || !pointer_over_ui_or_reference)
            .map(|center| BrushCursor {
                center,
                diameter: gui.brush.size,
            })
            .or_else(|| {
                cursor_pos
                    .filter(|_| !pointer_over_ui_or_reference)
                    .map(|center| BrushCursor {
                        center,
                        diameter: gui.brush.radius(brush_pressure) * 2.0,
                    })
            });
        let repaint_delay = ui::repaint_delay(&full_output);
        gui.state
            .handle_platform_output(window, full_output.platform_output);
        if reference_resize_active {
            window.set_cursor(CursorIcon::NwseResize);
        } else if reference_drag_active || is_panning || is_rotating_canvas {
            window.set_cursor(CursorIcon::Grabbing);
        } else if is_pan_modifier_active && !pointer_over_ui_or_reference {
            window.set_cursor(CursorIcon::Grab);
        }
        let eyedropper_over_canvas = is_eyedropper_active && !pointer_over_ui_or_reference;
        window.set_cursor_visible(
            is_resizing_brush || (brush_cursor.is_none() && !eyedropper_over_canvas),
        );

        for (id, image_delta) in &full_output.textures_delta.set {
            gui.renderer
                .update_texture(paint.device(), paint.queue(), *id, image_delta);
        }

        let paint_jobs = gui
            .context
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let frame = match paint.acquire_frame() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                paint.reconfigure_surface();
                return None;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return None,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = paint
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        paint.render_to_view(&mut encoder, &view, brush_cursor);
        let canvas_needs_redraw = paint.has_pending_stamps();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: paint.surface_size(),
            pixels_per_point: full_output.pixels_per_point,
        };
        let user_cmd_bufs = gui.renderer.update_buffers(
            paint.device(),
            paint.queue(),
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            gui.renderer
                .render(&mut pass, &paint_jobs, &screen_descriptor);
        }

        paint.queue().submit(
            user_cmd_bufs
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();

        for id in &full_output.textures_delta.free {
            gui.renderer.free_texture(id);
        }

        Some(RenderOutcome {
            repaint_delay,
            canvas_needs_redraw,
        })
    }

    pub(super) fn apply_pending_brush_change(&mut self) -> bool {
        let Some(change) = self.settings.take_pending_brush_change() else {
            return false;
        };
        let Some(paint) = self.paint.as_mut() else {
            self.settings.restore_pending_brush_change(change);
            return false;
        };
        let tool = change.tool;
        let reset_size = change.reset_size;
        match paint.try_set_brush_preset(&change.brush) {
            Ok(false) => {
                self.settings.restore_pending_brush_change(change);
                false
            }
            Ok(true) => {
                let completed = self.settings.complete_brush_change(change);
                let Some(gui) = self.gui.as_mut() else {
                    return true;
                };
                gui.apply_brush_preset(
                    tool,
                    self.settings.active_brush(),
                    completed.catalog,
                    completed.reloaded,
                    reset_size,
                );
                if completed.reloaded {
                    gui.settings_reloaded(self.settings.config(), tool);
                }
                if !completed.warnings.is_empty() {
                    gui.show_error(
                        "The selected brush could not be loaded completely.",
                        completed.warnings.join("\n"),
                    );
                }
                true
            }
            Err(error) => {
                if let Some(gui) = self.gui.as_mut() {
                    gui.show_error("Chromazen couldn’t load the selected brush.", error);
                }
                false
            }
        }
    }

    pub(super) fn update_repaint_schedule(
        &mut self,
        repaint_delay: Duration,
        window: &Window,
        force_immediate: bool,
    ) {
        if force_immediate || repaint_delay.is_zero() {
            self.next_repaint = None;
            window.request_redraw();
        } else if repaint_delay == Duration::MAX {
            self.next_repaint = None;
        } else {
            self.next_repaint = Instant::now().checked_add(repaint_delay);
        }
    }

    pub(super) fn request_scheduled_redraw(&mut self, event_loop: &ActiveEventLoop) {
        self.next_repaint = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    pub(super) fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        let next_repaint = match (
            self.next_repaint,
            (self.screen == AppScreen::Editor)
                .then(|| self.autosave.next_deadline())
                .flatten(),
        ) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        let Some(next_repaint) = next_repaint else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        if next_repaint <= Instant::now() {
            self.request_scheduled_redraw(event_loop);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_repaint));
        }
    }
}

