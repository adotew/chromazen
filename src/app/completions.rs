use super::*;

impl App {
    pub(super) fn apply_duplicate_completion(&mut self) -> bool {
        let Some(result) = self.gallery.take_duplicate_completion() else {
            return false;
        };
        if let Err(error) = result
            && let Some(gui) = self.gui.as_mut()
        {
            gui.open_error_dialog("Chromazen couldn’t duplicate the artwork.", error);
        }
        true
    }

    pub(super) fn apply_brush_import_completion(&mut self) -> bool {
        let Some(completion) = self.brush_import.take_completion() else {
            return false;
        };
        let imported_count = completion.imported_ids.len();
        if let Some(first_id) = completion.imported_ids.first() {
            self.handle_settings_commands(vec![SettingsCommand::SwitchBrush {
                tool: completion.tool,
                id: first_id.clone(),
                reset_size: true,
            }]);
        }

        let mut details = completion.warnings;
        details.extend(completion.errors);
        if let Some(gui) = self.gui.as_mut() {
            if imported_count == 0 {
                gui.open_error_dialog("No brushes were imported.", details.join("\n"));
            } else if details.is_empty() {
                gui.open_success_dialog(format!(
                    "Imported {imported_count} Photoshop brush{}",
                    if imported_count == 1 { "" } else { "es" }
                ));
            } else {
                gui.open_error_dialog(
                    format!(
                        "Imported {imported_count} Photoshop brush{}, but some brushes were skipped.",
                        if imported_count == 1 { "" } else { "es" },
                    ),
                    details.join("\n"),
                );
            }
        }
        true
    }

    pub(super) fn apply_reference_import_completions(&mut self) -> bool {
        let mut changed = false;
        while let Some(completion) = self.reference_import.take_completion() {
            changed = true;
            if self.autosave.artwork_id() != Some(&completion.artwork_id) {
                continue;
            }
            let base = completion.placement.unwrap_or_else(|| {
                self.paint.as_ref().map_or([100.0, 100.0], |paint| {
                    [paint.document_size()[0] as f32 + 100.0, 100.0]
                })
            });
            for (index, image) in completion.images.into_iter().enumerate() {
                let offset = index as f32 * 32.0;
                self.references
                    .add(image, [base[0] + offset, base[1] + offset]);
            }
            if !completion.errors.is_empty()
                && let Some(gui) = self.gui.as_mut()
            {
                gui.open_error_dialog(
                    "Some reference images could not be imported.",
                    completion.errors.join("\n"),
                );
            }
        }
        if changed {
            self.sync_history_menu();
        }
        changed
    }

    pub(super) fn apply_reference_load_completions(&mut self) -> bool {
        let mut changed = false;
        while let Some(completion) = self.reference_load.take_completion() {
            changed = true;
            let Some(pending) = self
                .pending_reference_load
                .take_if(|pending| pending.id == completion.artwork_id)
            else {
                continue;
            };
            self.references
                .replace_with_loaded_references(completion.references);
            self.autosave.begin_loaded_session(
                pending.id,
                pending.title,
                pending.paint_versions,
                self.references.versions(),
                pending.brush_color,
            );
            if !completion.warnings.is_empty()
                && let Some(gui) = self.gui.as_mut()
            {
                gui.open_error_dialog(
                    "Some reference images could not be loaded.",
                    completion.warnings.join("\n"),
                );
            }
        }
        changed
    }

    pub(super) fn apply_export_completion(&mut self) -> bool {
        let Some(completion) = self.export.take_completion() else {
            return false;
        };
        if let Some(gui) = self.gui.as_mut() {
            match completion.result {
                Ok(()) => gui
                    .open_success_dialog(format!("Exported PNG to {}", completion.path.display())),
                Err(error) => {
                    self.pending_exit = false;
                    gui.open_error_dialog("Chromazen couldn’t export the PNG.", error);
                }
            }
        }
        self.sync_history_menu();
        true
    }
}
