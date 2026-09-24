use super::*;

const FROST_BLUR_SUPPORT_PIXELS: f32 = 96.0;

struct EguiPaintBatch {
    range: std::ops::Range<usize>,
    refresh_frost: bool,
}

fn frost_refresh_required(
    surface_index: usize,
    surface: &ui::FrostedSurface,
    changed_rects: &[egui::Rect],
    pixels_per_point: f32,
) -> bool {
    if surface_index == 0 {
        return true;
    }
    let blur_support = FROST_BLUR_SUPPORT_PIXELS / pixels_per_point.max(0.01);
    let affected_rect = surface.rect.expand(blur_support);
    changed_rects
        .iter()
        .any(|changed| affected_rect.intersects(*changed))
}

fn tessellate_frost_batches(
    context: &egui::Context,
    shapes: Vec<egui::epaint::ClippedShape>,
    pixels_per_point: f32,
    surfaces: &[ui::FrostedSurface],
) -> (Vec<egui::ClippedPrimitive>, Vec<EguiPaintBatch>) {
    let mut shape_batches = Vec::new();
    let mut current_shapes = Vec::new();
    let mut current_refresh = false;
    let mut next_surface = 0;
    let mut changed_rects = Vec::new();

    for (shape_index, shape) in shapes.into_iter().enumerate() {
        if let Some(surface) = surfaces
            .get(next_surface)
            .filter(|surface| surface.shape_index == shape_index)
        {
            if !current_shapes.is_empty() {
                shape_batches.push((std::mem::take(&mut current_shapes), current_refresh));
            }
            current_refresh =
                frost_refresh_required(next_surface, surface, &changed_rects, pixels_per_point);
            if current_refresh {
                changed_rects.clear();
            }
            next_surface += 1;
        }
        let changed_rect = shape
            .clip_rect
            .intersect(shape.shape.visual_bounding_rect());
        if changed_rect.is_positive() {
            changed_rects.push(changed_rect);
        }
        current_shapes.push(shape);
    }
    if !current_shapes.is_empty() {
        shape_batches.push((current_shapes, current_refresh));
    }

    let mut paint_jobs = Vec::new();
    let mut paint_batches = Vec::with_capacity(shape_batches.len());
    for (shapes, refresh_frost) in shape_batches {
        let start = paint_jobs.len();
        paint_jobs.extend(context.tessellate(shapes, pixels_per_point));
        paint_batches.push(EguiPaintBatch {
            range: start..paint_jobs.len(),
            refresh_frost,
        });
    }
    (paint_jobs, paint_batches)
}

fn render_egui_batch(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    renderer: &mut egui_wgpu::Renderer,
    paint_jobs: &[egui::ClippedPrimitive],
    screen_descriptor: &ScreenDescriptor,
) {
    if paint_jobs.is_empty() {
        return;
    }
    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("egui pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
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
    renderer.render(&mut pass, paint_jobs, screen_descriptor);
}

impl App {
    pub(super) fn redraw(&mut self, window: &Window, event_loop: &ActiveEventLoop) {
        let mut app_action_processed = self.apply_export_completion();
        app_action_processed |= self.apply_duplicate_completion();
        app_action_processed |= self.apply_artwork_load_completion();
        app_action_processed |= self.apply_thumbnail_completions();
        app_action_processed |= self.apply_brush_import_completion();
        app_action_processed |= self.apply_reference_import_completions();
        app_action_processed |= self.apply_reference_load_completions();
        app_action_processed |= self.dispatch_pending_commands();
        let mut brush_switched = self.apply_pending_brush_change();

        if self.pending_exit && self.screen == AppScreen::Empty && !self.export.is_exporting() {
            event_loop.exit();
            return;
        }
        if self.screen == AppScreen::Editor {
            if let Some(paint) = self.paint.as_ref() {
                app_action_processed |= self.autosave.update(paint, &self.references);
            }
            if self.autosave.take_catalog_dirty() {
                self.gallery.refresh();
                app_action_processed = true;
            }
            if self.pending_exit
                && !self.reference_load.is_loading()
                && self
                    .paint
                    .as_ref()
                    .is_some_and(|paint| self.autosave.is_clean(paint, &self.references))
                && !self.export.is_exporting()
            {
                event_loop.exit();
                return;
            }
            app_action_processed |= self.apply_pending_artwork();
        }

        if self
            .gpu
            .as_ref()
            .is_none_or(|gpu| gpu.surface_size()[0] == 0 || gpu.surface_size()[1] == 0)
        {
            return;
        }
        let layer_content_bounds = (self.screen == AppScreen::Editor
            && self.input.tool().paint_tool().is_none())
        .then(|| {
            self.paint
                .as_mut()
                .and_then(Canvas::read_selected_layer_content_bounds)
        })
        .flatten();
        let Some(paint) = self.paint.as_ref() else {
            return;
        };
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };

        let menu = self.application_menu_state();
        let (full_output, commands) = {
            let Some(gui) = self.gui.as_mut() else {
                return;
            };
            let warning = self.gallery.warning();
            let output = match self.screen {
                AppScreen::Empty => gui.run_empty(
                    window,
                    self.gallery.artworks(),
                    warning.as_deref(),
                    self.gallery.load_dialog_delay(),
                ),
                AppScreen::Editor => {
                    gui.sync_layer_thumbnails(paint, gpu.device());
                    let layer_snapshot = paint.layer_snapshot();
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
                    } else {
                        self.pending_artwork.as_ref().map(|pending| match pending {
                            PendingArtwork::Open(_) => "Switching Artwork",
                            PendingArtwork::Create(_) => "Creating New Artwork",
                            PendingArtwork::Duplicate(_) => "Duplicating Artwork",
                            PendingArtwork::Delete(_) => "Deleting Artwork",
                        })
                    };
                    let Some(active_artwork_id) = self
                        .autosave
                        .artwork_id()
                        .or_else(|| self.pending_reference_load.as_ref().map(|load| &load.id))
                    else {
                        return;
                    };
                    let Some(active_artwork_title) = self.autosave.artwork_title().or_else(|| {
                        self.pending_reference_load
                            .as_ref()
                            .map(|load| load.title.as_str())
                    }) else {
                        return;
                    };
                    gui.run_editor(
                        window,
                        EditorUiState {
                            menu,
                            artworks: self.gallery.artworks(),
                            active_artwork_id,
                            active_artwork_title,
                            active_artwork_dimensions: paint.document_size(),
                            artwork_warning: warning.as_deref(),
                            artwork_load_dialog_delay: self.gallery.load_dialog_delay(),
                            layers: &layer_snapshot,
                            tool: self.input.tool(),
                            layer_transform: paint.active_layer_transform(),
                            layer_content_bounds,
                            brush_resize_position: self.input.brush_resize_pos(),
                            brush_outline_half_size: &|size| paint.brush_outline_half_size(size),
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
        app_action_processed |= self.dispatch_pending_commands();

        if self.screen == AppScreen::Editor
            && let Some(paint) = self.paint.as_ref()
        {
            app_action_processed |= self.autosave.update(paint, &self.references);
        }
        if self.autosave.take_catalog_dirty() {
            self.gallery.refresh();
            app_action_processed = true;
        }
        let Some(outcome) = self.render_and_present_frame(window, full_output) else {
            return;
        };
        brush_switched |= self.apply_pending_brush_change();
        self.update_repaint_schedule(
            outcome.repaint_delay,
            window,
            outcome.canvas_needs_redraw || app_action_processed || brush_switched,
        );
    }

    pub(super) fn render_and_present_frame(
        &mut self,
        window: &Window,
        mut full_output: egui::FullOutput,
    ) -> Option<RenderOutcome> {
        let cursor_pos = self.input.brush_cursor_pos();
        let is_resizing_brush = self.input.is_resizing_brush();
        let is_panning = self.input.is_panning();
        let is_rotating_canvas = self.input.is_rotating_canvas();
        let is_pan_modifier_active = self.input.is_pan_modifier_active();
        let is_eyedropper_active = self.input.is_eyedropper_active();
        let brush_pressure = self.pressure_state.brush_pressure();
        let gpu = self.gpu.as_ref()?;
        let paint = self.paint.as_mut()?;
        let gui = self.gui.as_mut()?;
        gui.sync_frost_texture(gpu);
        let frosted_surfaces = gui.prepare_frosted_surfaces(&mut full_output.shapes);
        let frost_visible = !frosted_surfaces.is_empty();
        let pointer_over_ui = gui.context.is_pointer_over_egui();
        let pointer_over_reference =
            self.screen == AppScreen::Editor && gui.pointer_over_reference();
        let reference_drag_active = self.screen == AppScreen::Editor && gui.reference_drag_active();
        let reference_resize_active =
            self.screen == AppScreen::Editor && gui.reference_resize_active();
        let pointer_over_ui_or_reference =
            pointer_over_ui || pointer_over_reference || reference_drag_active;
        let brush_cursor = gui.brush_adjustment_preview().or_else(|| {
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
        } else if is_pan_modifier_active && (!pointer_over_ui || pointer_over_reference) {
            window.set_cursor(CursorIcon::Grab);
        }
        let eyedropper_over_canvas = is_eyedropper_active && !pointer_over_ui_or_reference;
        window.set_cursor_visible(
            is_resizing_brush
                || gui.brush_slider_active()
                || (brush_cursor.is_none() && !eyedropper_over_canvas),
        );

        for (id, image_delta) in &full_output.textures_delta.set {
            gui.renderer
                .update_texture(gpu.device(), gpu.queue(), *id, image_delta);
        }

        let (mut paint_jobs, paint_batches) = tessellate_frost_batches(
            &gui.context,
            full_output.shapes,
            full_output.pixels_per_point,
            &frosted_surfaces,
        );
        let frame = match gpu.acquire_frame() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                gpu.reconfigure_surface();
                return None;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return None,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        let fallback_view = frost_visible.then(|| gpu.fallback_frame_view()).flatten();
        let render_view = fallback_view.unwrap_or(&view);
        paint.set_workspace_background_color(gui.workspace_background_color());
        paint.render_to_view(&mut encoder, render_view, brush_cursor);
        let canvas_needs_redraw = paint.has_pending_stamps();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: gpu.surface_size(),
            pixels_per_point: full_output.pixels_per_point,
        };
        let user_cmd_bufs = gui.renderer.update_buffers(
            gpu.device(),
            gpu.queue(),
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        if frost_visible {
            // Refresh between overlapping surfaces so each panel samples the UI already painted
            // below it. Non-overlapping panels reuse the same quarter-resolution blur. The egui
            // renderer starts its buffer-slice iterators at zero on every call, so retain earlier
            // jobs with empty clips to advance those iterators without drawing them again.
            for batch in &paint_batches {
                if batch.refresh_frost {
                    gpu.render_frost(&mut encoder, render_view);
                }
                render_egui_batch(
                    &mut encoder,
                    render_view,
                    &mut gui.renderer,
                    &paint_jobs[..batch.range.end],
                    &screen_descriptor,
                );
                for job in &mut paint_jobs[batch.range.clone()] {
                    job.clip_rect = egui::Rect::ZERO;
                }
            }
        } else {
            render_egui_batch(
                &mut encoder,
                render_view,
                &mut gui.renderer,
                &paint_jobs,
                &screen_descriptor,
            );
        }
        if fallback_view.is_some() {
            gpu.blit_fallback_frame(&mut encoder, &view);
        }

        gpu.queue().submit(
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
        match paint.try_set_brush_stamp(&change.brush.stamp_image) {
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
                    gui.apply_reloaded_settings(self.settings.config(), tool);
                }
                if !completed.warnings.is_empty() {
                    gui.open_error_dialog(
                        "The selected brush could not be loaded completely.",
                        completed.warnings.join("\n"),
                    );
                }
                true
            }
            Err(error) => {
                if let Some(gui) = self.gui.as_mut() {
                    gui.open_error_dialog("Chromazen couldn’t load the selected brush.", error);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frost_is_refreshed_only_for_content_near_the_surface() {
        let first = ui::FrostedSurface {
            shape_index: 0,
            rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
        };
        let second = ui::FrostedSurface {
            shape_index: 2,
            rect: egui::Rect::from_min_max(egui::pos2(400.0, 0.0), egui::pos2(500.0, 100.0)),
        };
        let reference = egui::Rect::from_min_max(egui::pos2(420.0, 20.0), egui::pos2(480.0, 80.0));
        let distant = egui::Rect::from_min_max(egui::pos2(700.0, 20.0), egui::pos2(760.0, 80.0));

        assert!(frost_refresh_required(0, &first, &[], 2.0));
        assert!(frost_refresh_required(1, &second, &[reference], 2.0));
        assert!(!frost_refresh_required(1, &second, &[distant], 2.0));
    }

    #[test]
    fn frost_is_refreshed_for_nearby_stacked_surfaces() {
        let first = ui::FrostedSurface {
            shape_index: 0,
            rect: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
        };
        let second = ui::FrostedSurface {
            shape_index: 1,
            rect: egui::Rect::from_min_max(egui::pos2(150.0, 0.0), egui::pos2(250.0, 100.0)),
        };

        assert!(frost_refresh_required(1, &second, &[first.rect], 1.0));
        assert!(!frost_refresh_required(1, &second, &[first.rect], 2.0));
    }
}
