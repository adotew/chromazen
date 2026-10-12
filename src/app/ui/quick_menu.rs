use super::*;

const QUICK_MENU_RADIUS: f32 = 100.0;
const QUICK_MENU_SLOT_RADIUS: f32 = 66.0;
const QUICK_MENU_DEAD_ZONE: f32 = 28.0;
const QUICK_MENU_REACH: f32 = 160.0;
const QUICK_MENU_HIGHLIGHT_RADIUS: f32 = 22.0;
const QUICK_MENU_ICON_SIZE: f32 = 22.0;
const QUICK_MENU_SCREEN_MARGIN: f32 = 8.0;
const QUICK_MENU_OPEN_SECONDS: f32 = 0.12;
const QUICK_MENU_CLOSE_SECONDS: f32 = 0.09;
const QUICK_MENU_HOVER_SECONDS: f32 = 0.06;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickAction {
    Brush,
    Eraser,
    Redo,
    AddLayer,
    ClearLayer,
    FlipCanvas,
    Undo,
    Smudge,
}

/// Slots run clockwise from the top.
const QUICK_ACTIONS: [QuickAction; 8] = [
    QuickAction::Brush,
    QuickAction::Eraser,
    QuickAction::Redo,
    QuickAction::AddLayer,
    QuickAction::ClearLayer,
    QuickAction::FlipCanvas,
    QuickAction::Undo,
    QuickAction::Smudge,
];

impl QuickAction {
    fn label(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Eraser => "Eraser",
            Self::Redo => "Redo",
            Self::AddLayer => "New layer",
            Self::ClearLayer => "Clear layer",
            Self::FlipCanvas => "Flip canvas",
            Self::Undo => "Undo",
            Self::Smudge => "Smudge",
        }
    }

    fn icon(self) -> egui::ImageSource<'static> {
        match self {
            Self::Brush => egui::include_image!("../../../assets/icons/brush.svg"),
            Self::Eraser => egui::include_image!("../../../assets/icons/eraser.svg"),
            Self::Redo => egui::include_image!("../../../assets/icons/redo-2.svg"),
            Self::AddLayer => egui::include_image!("../../../assets/icons/layers-plus.svg"),
            Self::ClearLayer => egui::include_image!("../../../assets/icons/brush-cleaning.svg"),
            Self::FlipCanvas => egui::include_image!(
                "../../../assets/icons/square-centerline-dashed-horizontal.svg"
            ),
            Self::Undo => egui::include_image!("../../../assets/icons/undo-2.svg"),
            Self::Smudge => egui::include_image!("../../../assets/icons/waves-horizontal.svg"),
        }
    }

    fn tool(self) -> Option<EditorTool> {
        match self {
            Self::Brush => Some(PaintTool::Brush.into()),
            Self::Eraser => Some(PaintTool::Eraser.into()),
            Self::Smudge => Some(PaintTool::Smudge.into()),
            _ => None,
        }
    }

    fn command(self) -> EditorCommand {
        match self {
            Self::Brush => EditorCommand::SelectTool(PaintTool::Brush.into()),
            Self::Eraser => EditorCommand::SelectTool(PaintTool::Eraser.into()),
            Self::Redo => EditorCommand::Redo,
            Self::AddLayer => EditorCommand::AddLayer,
            Self::ClearLayer => EditorCommand::ClearLayer,
            Self::FlipCanvas => EditorCommand::ToggleCanvasFlipHorizontal,
            Self::Undo => EditorCommand::Undo,
            Self::Smudge => EditorCommand::SelectTool(PaintTool::Smudge.into()),
        }
    }

    fn is_enabled(self, menu: ApplicationMenuState) -> bool {
        match self {
            Self::Undo => menu.can_undo,
            Self::Redo => menu.can_redo,
            _ => true,
        }
    }
}

#[derive(Debug)]
pub(super) struct QuickMenu {
    /// Logical egui points.
    center: egui::Pos2,
    open: bool,
    held_by_key: bool,
    key_released: bool,
    chosen: Option<usize>,
    progress: f32,
    hover: [f32; QUICK_ACTIONS.len()],
}

impl GuiLayer {
    /// Opens the menu around a window position in physical pixels. Opening it again from the key
    /// closes it, so a second `Q` press dismisses a menu left open after a tap.
    pub(crate) fn open_quick_menu(&mut self, position: [f32; 2], held_by_key: bool) {
        if let Some(menu) = self.quick_menu.as_mut().filter(|menu| menu.open) {
            if held_by_key {
                menu.open = false;
                self.context.request_repaint();
            }
            return;
        }
        let pixels_per_point = self.context.pixels_per_point();
        let pointer = egui::pos2(
            position[0] / pixels_per_point,
            position[1] / pixels_per_point,
        );
        self.quick_menu = Some(QuickMenu {
            center: quick_menu_center(pointer, self.context.content_rect()),
            open: true,
            held_by_key,
            key_released: false,
            chosen: None,
            progress: 0.0,
            hover: [0.0; QUICK_ACTIONS.len()],
        });
        self.context.request_repaint();
    }

    pub(crate) fn release_quick_menu_key(&mut self) {
        if let Some(menu) = self.quick_menu.as_mut().filter(|menu| menu.held_by_key) {
            menu.key_released = true;
            self.context.request_repaint();
        }
    }

    pub(super) fn close_quick_menu(&mut self) -> bool {
        let Some(menu) = self.quick_menu.as_mut().filter(|menu| menu.open) else {
            return false;
        };
        menu.open = false;
        self.context.request_repaint();
        true
    }

    pub(super) fn show_quick_menu(
        &mut self,
        ctx: &egui::Context,
        menu_state: ApplicationMenuState,
        tool: EditorTool,
    ) {
        let Some(menu) = self.quick_menu.as_mut() else {
            return;
        };
        let (pointer, primary_pressed, secondary_pressed, dt) = ctx.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_pressed(),
                input.pointer.secondary_pressed(),
                input.stable_dt.min(0.1),
            )
        });
        let hovered = if menu.open {
            pointer
                .and_then(|pointer| quick_menu_slot_at(pointer - menu.center))
                .filter(|&slot| QUICK_ACTIONS[slot].is_enabled(menu_state))
        } else {
            menu.chosen
        };

        if menu.open {
            let released_key = std::mem::take(&mut menu.key_released);
            if released_key {
                menu.held_by_key = false;
            }
            if primary_pressed || (released_key && hovered.is_some()) {
                menu.open = false;
                menu.chosen = hovered;
                if let Some(slot) = hovered {
                    self.commands
                        .push(AppCommand::Editor(QUICK_ACTIONS[slot].command()));
                }
            } else if secondary_pressed {
                menu.open = false;
            }
        }

        let duration = if menu.open {
            QUICK_MENU_OPEN_SECONDS
        } else {
            QUICK_MENU_CLOSE_SECONDS
        };
        menu.progress = step_toward(menu.progress, f32::from(menu.open), dt, duration);
        let mut animating = menu.progress != f32::from(menu.open);
        for (slot, hover) in menu.hover.iter_mut().enumerate() {
            let target = f32::from(hovered == Some(slot));
            *hover = step_toward(*hover, target, dt, QUICK_MENU_HOVER_SECONDS);
            animating |= *hover != target;
        }
        if !menu.open && menu.progress == 0.0 {
            self.quick_menu = None;
            return;
        }
        if animating {
            ctx.request_repaint();
        }

        let screen = ctx.content_rect();
        let open = menu.open;
        let center = menu.center;
        let progress = egui::emath::easing::cubic_out(menu.progress);
        let hover = menu.hover;
        let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("quick menu"));
        // Clicked toolbars rise to the top of the foreground; keep the open menu above them.
        ctx.move_to_top(layer_id);
        egui::Area::new(layer_id.id)
            .order(layer_id.order)
            .fixed_pos(screen.min)
            .default_size(screen.size())
            .interactable(open)
            .show(ctx, |ui| {
                // While open, the menu covers the window so clicks around it dismiss the menu
                // instead of painting.
                let sense = if open {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                ui.allocate_exact_size(screen.size(), sense);
                paint_quick_menu(ui, center, progress, &hover, menu_state, tool);
            });
    }
}

fn paint_quick_menu(
    ui: &egui::Ui,
    center: egui::Pos2,
    progress: f32,
    hover: &[f32; QUICK_ACTIONS.len()],
    menu_state: ApplicationMenuState,
    tool: EditorTool,
) {
    let visuals = ui.visuals();
    let accent = visuals.selection.bg_fill;
    let on_accent = visuals.selection.stroke.color;
    let text_color = visuals.text_color();
    let radius = QUICK_MENU_RADIUS * progress;
    // A fully rounded surface-colored rect, rather than a circle shape, keeps frosted surfaces
    // blurring the canvas behind the menu.
    ui.painter().add(egui::epaint::RectShape::new(
        egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0)),
        egui::CornerRadius::same(radius.round() as u8),
        visuals.window_fill(),
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    ));

    let content_alpha = progress * progress;
    for (slot, action) in QUICK_ACTIONS.into_iter().enumerate() {
        let slot_center =
            center + quick_menu_slot_direction(slot) * QUICK_MENU_SLOT_RADIUS * progress;
        let hover = egui::emath::easing::cubic_out(hover[slot]);
        if hover > 0.0 {
            ui.painter().circle_filled(
                slot_center,
                QUICK_MENU_HIGHLIGHT_RADIUS * hover * progress,
                accent.gamma_multiply(content_alpha),
            );
        }
        let resting = if !action.is_enabled(menu_state) {
            visuals.weak_text_color().gamma_multiply(0.5)
        } else if action.tool() == Some(tool) {
            accent
        } else {
            text_color
        };
        let tint = resting.lerp_to_gamma(on_accent, hover);
        egui::Image::new(action.icon())
            .tint(tint.gamma_multiply(content_alpha))
            .alt_text(action.label())
            .paint_at(
                ui,
                egui::Rect::from_center_size(
                    slot_center,
                    egui::Vec2::splat(QUICK_MENU_ICON_SIZE * progress),
                ),
            );
    }

    if let Some((slot, hover)) = hover
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, hover)| *hover > 0.0)
    {
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            QUICK_ACTIONS[slot].label(),
            egui::FontId::proportional(13.0),
            text_color.gamma_multiply(content_alpha * hover),
        );
    }
}

/// Unit vector from the menu center to a slot, in egui's y-down coordinates.
fn quick_menu_slot_direction(slot: usize) -> egui::Vec2 {
    let angle = slot as f32 * std::f32::consts::TAU / QUICK_ACTIONS.len() as f32;
    egui::vec2(angle.sin(), -angle.cos())
}

/// Picks a slot by direction alone so wide wedges are easy to hit with a pen. Points in the
/// center dead zone or far outside the ring select nothing.
fn quick_menu_slot_at(offset: egui::Vec2) -> Option<usize> {
    let distance = offset.length();
    if !(QUICK_MENU_DEAD_ZONE..=QUICK_MENU_REACH).contains(&distance) {
        return None;
    }
    let slot_angle = std::f32::consts::TAU / QUICK_ACTIONS.len() as f32;
    let angle = offset.x.atan2(-offset.y).rem_euclid(std::f32::consts::TAU);
    Some(((angle + slot_angle / 2.0) / slot_angle) as usize % QUICK_ACTIONS.len())
}

/// Keeps the whole ring inside the window, centering it when the window is too small.
fn quick_menu_center(pointer: egui::Pos2, screen: egui::Rect) -> egui::Pos2 {
    let inset = QUICK_MENU_RADIUS + QUICK_MENU_SCREEN_MARGIN;
    let clamp = |value: f32, min: f32, max: f32| {
        if min > max {
            (min + max) / 2.0
        } else {
            value.clamp(min, max)
        }
    };
    egui::pos2(
        clamp(pointer.x, screen.left() + inset, screen.right() - inset),
        clamp(pointer.y, screen.top() + inset, screen.bottom() - inset),
    )
}

fn step_toward(value: f32, target: f32, dt: f32, duration: f32) -> f32 {
    let step = dt / duration;
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset_at(degrees: f32, distance: f32) -> egui::Vec2 {
        let radians = degrees.to_radians();
        egui::vec2(radians.sin(), -radians.cos()) * distance
    }

    #[test]
    fn quick_menu_slots_run_clockwise_from_the_top() {
        for slot in 0..QUICK_ACTIONS.len() {
            let offset = quick_menu_slot_direction(slot) * QUICK_MENU_SLOT_RADIUS;
            assert_eq!(quick_menu_slot_at(offset), Some(slot));
        }
        assert_eq!(quick_menu_slot_at(egui::vec2(0.0, -60.0)), Some(0));
        assert_eq!(quick_menu_slot_at(egui::vec2(60.0, 0.0)), Some(2));
        assert_eq!(quick_menu_slot_at(egui::vec2(0.0, 60.0)), Some(4));
        assert_eq!(quick_menu_slot_at(egui::vec2(-60.0, 0.0)), Some(6));
    }

    #[test]
    fn quick_menu_slot_boundaries_wrap_around_the_top() {
        assert_eq!(quick_menu_slot_at(offset_at(22.0, 60.0)), Some(0));
        assert_eq!(quick_menu_slot_at(offset_at(23.0, 60.0)), Some(1));
        assert_eq!(quick_menu_slot_at(offset_at(337.0, 60.0)), Some(7));
        assert_eq!(quick_menu_slot_at(offset_at(338.0, 60.0)), Some(0));
        assert_eq!(quick_menu_slot_at(offset_at(-10.0, 60.0)), Some(0));
    }

    #[test]
    fn quick_menu_center_and_far_points_select_nothing() {
        assert_eq!(quick_menu_slot_at(egui::Vec2::ZERO), None);
        assert_eq!(
            quick_menu_slot_at(offset_at(90.0, QUICK_MENU_DEAD_ZONE - 1.0)),
            None
        );
        assert_eq!(
            quick_menu_slot_at(offset_at(90.0, QUICK_MENU_REACH + 1.0)),
            None
        );
        assert_eq!(
            quick_menu_slot_at(offset_at(90.0, QUICK_MENU_REACH)),
            Some(2)
        );
    }

    #[test]
    fn quick_menu_stays_inside_the_window() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let inset = QUICK_MENU_RADIUS + QUICK_MENU_SCREEN_MARGIN;

        assert_eq!(
            quick_menu_center(egui::pos2(400.0, 300.0), screen),
            egui::pos2(400.0, 300.0)
        );
        assert_eq!(
            quick_menu_center(egui::pos2(5.0, 590.0), screen),
            egui::pos2(inset, 600.0 - inset)
        );

        let small = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(150.0, 90.0));
        assert_eq!(
            quick_menu_center(egui::pos2(0.0, 0.0), small),
            small.center()
        );
    }

    #[test]
    fn quick_actions_dispatch_existing_editor_commands() {
        let commands = QUICK_ACTIONS.map(QuickAction::command);
        assert_eq!(
            commands,
            [
                EditorCommand::SelectTool(PaintTool::Brush.into()),
                EditorCommand::SelectTool(PaintTool::Eraser.into()),
                EditorCommand::Redo,
                EditorCommand::AddLayer,
                EditorCommand::ClearLayer,
                EditorCommand::ToggleCanvasFlipHorizontal,
                EditorCommand::Undo,
                EditorCommand::SelectTool(PaintTool::Smudge.into()),
            ]
        );
    }

    #[test]
    fn unavailable_history_actions_are_disabled() {
        let menu = ApplicationMenuState::new(true, false, false, true, false);
        assert!(!QuickAction::Undo.is_enabled(menu));
        assert!(QuickAction::Redo.is_enabled(menu));
        assert!(QuickAction::ClearLayer.is_enabled(menu));
    }

    #[test]
    fn animation_steps_reach_their_target_without_overshooting() {
        assert_eq!(step_toward(0.0, 1.0, 0.06, 0.12), 0.5);
        assert_eq!(step_toward(0.9, 1.0, 0.06, 0.12), 1.0);
        assert_eq!(step_toward(0.1, 0.0, 0.06, 0.09), 0.0);
        assert_eq!(step_toward(1.0, 1.0, 0.06, 0.12), 1.0);
    }
}
