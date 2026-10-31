use std::f32::consts::{PI, TAU};

use super::*;
use brush_panel::paint_stamp;
use layers_panel::{can_delete_layer, can_merge_layer_down};

const QUICK_MENU_RADIUS: f32 = 100.0;
const QUICK_MENU_SLOT_RADIUS: f32 = 66.0;
const QUICK_MENU_DEAD_ZONE: f32 = 28.0;
const QUICK_MENU_REACH: f32 = 160.0;
const QUICK_MENU_HIGHLIGHT_RADIUS: f32 = 22.0;
const QUICK_MENU_ICON_SIZE: f32 = 22.0;
/// Stamps have soft, transparent edges, so they need more room than icons to read at a glance.
const QUICK_MENU_STAMP_SIZE: f32 = 36.0;
/// Space between a highlighted item and the edge of the surface behind it, matching the ring.
const QUICK_MENU_EDGE_PADDING: f32 =
    QUICK_MENU_RADIUS - QUICK_MENU_SLOT_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
/// Group actions keep the same gap to the ring's highlights as to the band's outer edge.
const QUICK_MENU_GROUP_RADIUS: f32 = QUICK_MENU_RADIUS + QUICK_MENU_HIGHLIGHT_RADIUS;
const QUICK_MENU_BAND_WIDTH: f32 = 2.0 * QUICK_MENU_HIGHLIGHT_RADIUS + QUICK_MENU_EDGE_PADDING;
/// Covers the disc's border where the band joins it.
const QUICK_MENU_BAND_OVERLAP: f32 = 1.0;
const QUICK_MENU_BAND_CORNER_RADIUS: f32 = 12.0;
const QUICK_MENU_BAND_SEGMENT: f32 = 3.0 * PI / 180.0;
const QUICK_MENU_BAND_CORNER_STEPS: usize = 6;
const QUICK_MENU_GROUP_REACH: f32 = 210.0;
const QUICK_MENU_GROUP_SPACING: f32 = 24.0 * PI / 180.0;
const QUICK_MENU_GROUP_CAPACITY: usize = 5;
const QUICK_MENU_SCREEN_MARGIN: f32 = 8.0;
const QUICK_MENU_OPEN_SECONDS: f32 = 0.12;
const QUICK_MENU_CLOSE_SECONDS: f32 = 0.09;
const QUICK_MENU_GROUP_SECONDS: f32 = 0.08;
const QUICK_MENU_HOVER_SECONDS: f32 = 0.06;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickAction {
    Brush,
    Eraser,
    Smudge,
    Select,
    Undo,
    Redo,
    NewLayer,
    DuplicateLayer,
    MergeDown,
    ClearLayer,
    DeleteLayer,
    RotateRight,
    RotateLeft,
    FlipHorizontal,
    FlipVertical,
    ResetRotation,
    /// Index into the quick menu's brush list.
    Preset(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickGroup {
    Tool,
    Brush,
    Layer,
    Canvas,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickSlot {
    Action(QuickAction),
    Group(QuickGroup),
}

/// Slots run clockwise from the top.
const QUICK_SLOTS: [QuickSlot; 7] = [
    QuickSlot::Group(QuickGroup::Tool),
    QuickSlot::Group(QuickGroup::Brush),
    QuickSlot::Action(QuickAction::Redo),
    QuickSlot::Group(QuickGroup::Layer),
    QuickSlot::Group(QuickGroup::Canvas),
    QuickSlot::Action(QuickAction::Select),
    QuickSlot::Action(QuickAction::Undo),
];

/// A ring slot, or an action on the open group's arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuickTarget {
    Slot(usize),
    GroupAction(usize),
}

/// A brush preset of the active paint tool, listed on the brush arc.
#[derive(Clone, Debug)]
struct QuickBrush {
    tool: PaintTool,
    id: String,
    name: String,
    texture: Option<egui::TextureId>,
}

/// Editor state that decides which actions are available.
#[derive(Clone, Copy, Debug)]
struct QuickMenuContext {
    menu: ApplicationMenuState,
    tool: EditorTool,
    selected_brush: Option<usize>,
    selected_layer: LayerId,
    can_merge_down: bool,
    can_delete_layer: bool,
}

impl QuickMenuContext {
    fn new(menu: ApplicationMenuState, tool: EditorTool, layers: &LayerSnapshot) -> Self {
        let selected_index = layers
            .layers
            .iter()
            .position(|layer| layer.id == layers.selection);
        Self {
            menu,
            tool,
            selected_brush: None,
            selected_layer: layers.selection,
            can_merge_down: selected_index
                .is_some_and(|index| can_merge_layer_down(&layers.layers, index)),
            can_delete_layer: selected_index
                .is_some_and(|index| can_delete_layer(&layers.layers, index)),
        }
    }
}

impl QuickAction {
    fn label(self) -> &'static str {
        match self {
            Self::Brush => "Brush",
            Self::Eraser => "Eraser",
            Self::Smudge => "Smudge",
            Self::Select => "Select",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::NewLayer => "New layer",
            Self::DuplicateLayer => "Duplicate layer",
            Self::MergeDown => "Merge down",
            Self::ClearLayer => "Clear layer",
            Self::DeleteLayer => "Delete layer",
            Self::RotateRight => "Rotate right",
            Self::RotateLeft => "Rotate left",
            Self::FlipHorizontal => "Flip horizontal",
            Self::FlipVertical => "Flip vertical",
            Self::ResetRotation => "Reset rotation",
            Self::Preset(_) => "Brush",
        }
    }

    fn icon(self) -> egui::ImageSource<'static> {
        match self {
            Self::Brush => egui::include_image!("../../../assets/icons/brush.svg"),
            Self::Eraser => egui::include_image!("../../../assets/icons/eraser.svg"),
            Self::Smudge => egui::include_image!("../../../assets/icons/waves-horizontal.svg"),
            Self::Select => egui::include_image!("../../../assets/icons/lasso.svg"),
            Self::Undo => egui::include_image!("../../../assets/icons/undo-2.svg"),
            Self::Redo => egui::include_image!("../../../assets/icons/redo-2.svg"),
            Self::NewLayer => egui::include_image!("../../../assets/icons/layers-plus.svg"),
            Self::DuplicateLayer => egui::include_image!("../../../assets/icons/copy.svg"),
            Self::MergeDown => {
                egui::include_image!("../../../assets/icons/arrow-down-to-line.svg")
            }
            Self::ClearLayer => egui::include_image!("../../../assets/icons/brush-cleaning.svg"),
            Self::DeleteLayer => egui::include_image!("../../../assets/icons/trash-2.svg"),
            Self::RotateRight => egui::include_image!("../../../assets/icons/rotate-cw.svg"),
            Self::RotateLeft => egui::include_image!("../../../assets/icons/rotate-ccw.svg"),
            Self::FlipHorizontal => egui::include_image!(
                "../../../assets/icons/square-centerline-dashed-horizontal.svg"
            ),
            Self::FlipVertical => {
                egui::include_image!("../../../assets/icons/square-centerline-dashed-vertical.svg")
            }
            Self::ResetRotation => egui::include_image!("../../../assets/icons/compass.svg"),
            Self::Preset(_) => egui::include_image!("../../../assets/icons/paintbrush.svg"),
        }
    }

    fn tool(self) -> Option<EditorTool> {
        match self {
            Self::Brush => Some(PaintTool::Brush.into()),
            Self::Eraser => Some(PaintTool::Eraser.into()),
            Self::Smudge => Some(PaintTool::Smudge.into()),
            Self::Select => Some(EditorTool::Select),
            _ => None,
        }
    }

    fn command(self, selected_layer: LayerId, brushes: &[QuickBrush]) -> AppCommand {
        let command = match self {
            Self::Preset(index) => {
                let brush = &brushes[index];
                return AppCommand::Settings(SettingsCommand::SwitchBrush {
                    tool: brush.tool,
                    id: brush.id.clone(),
                });
            }
            Self::Brush => EditorCommand::SelectTool(PaintTool::Brush.into()),
            Self::Eraser => EditorCommand::SelectTool(PaintTool::Eraser.into()),
            Self::Smudge => EditorCommand::SelectTool(PaintTool::Smudge.into()),
            Self::Select => EditorCommand::SelectTool(EditorTool::Select),
            Self::Undo => EditorCommand::Undo,
            Self::Redo => EditorCommand::Redo,
            Self::NewLayer => EditorCommand::AddLayer,
            Self::DuplicateLayer => EditorCommand::DuplicateSelectedLayer,
            Self::MergeDown => EditorCommand::MergeLayerDown(selected_layer),
            Self::ClearLayer => EditorCommand::ClearLayer,
            Self::DeleteLayer => EditorCommand::DeleteSelectedLayer,
            Self::RotateRight => EditorCommand::RotateCanvasRight,
            Self::RotateLeft => EditorCommand::RotateCanvasLeft,
            Self::FlipHorizontal => EditorCommand::ToggleCanvasFlipHorizontal,
            Self::FlipVertical => EditorCommand::ToggleCanvasFlipVertical,
            Self::ResetRotation => EditorCommand::ResetCanvasRotation,
        };
        AppCommand::Editor(command)
    }

    fn is_enabled(self, context: QuickMenuContext) -> bool {
        match self {
            Self::Undo => context.menu.can_undo,
            Self::Redo => context.menu.can_redo,
            Self::MergeDown => context.can_merge_down,
            Self::DeleteLayer => context.can_delete_layer,
            Self::RotateRight
            | Self::RotateLeft
            | Self::FlipHorizontal
            | Self::FlipVertical
            | Self::ResetRotation => context.menu.canvas_enabled,
            _ => true,
        }
    }
}

impl QuickGroup {
    fn label(self) -> &'static str {
        match self {
            Self::Tool => "Tool",
            Self::Brush => "Brush",
            Self::Layer => "Layer",
            Self::Canvas => "Canvas",
        }
    }

    /// The tool group shows the active paint tool, falling back to the brush for other tools.
    fn icon(self, tool: EditorTool) -> egui::ImageSource<'static> {
        match self {
            Self::Tool => match tool.paint_tool() {
                Some(PaintTool::Eraser) => QuickAction::Eraser.icon(),
                Some(PaintTool::Smudge) => QuickAction::Smudge.icon(),
                _ => QuickAction::Brush.icon(),
            },
            Self::Brush => egui::include_image!("../../../assets/icons/paintbrush.svg"),
            Self::Layer => egui::include_image!("../../../assets/icons/layers.svg"),
            Self::Canvas => egui::include_image!("../../../assets/icons/frame.svg"),
        }
    }

    /// Actions run clockwise along the arc.
    fn actions(self, brushes: usize) -> Vec<QuickAction> {
        match self {
            Self::Tool => vec![QuickAction::Brush, QuickAction::Eraser, QuickAction::Smudge],
            Self::Brush => (0..brushes).map(QuickAction::Preset).collect(),
            Self::Layer => vec![
                QuickAction::NewLayer,
                QuickAction::DuplicateLayer,
                QuickAction::MergeDown,
                QuickAction::ClearLayer,
                QuickAction::DeleteLayer,
            ],
            // The arc opens below the ring, where clockwise runs right to left.
            Self::Canvas => vec![
                QuickAction::RotateRight,
                QuickAction::FlipHorizontal,
                QuickAction::ResetRotation,
                QuickAction::FlipVertical,
                QuickAction::RotateLeft,
            ],
        }
    }
}

impl QuickSlot {
    fn label(self) -> &'static str {
        match self {
            Self::Action(action) => action.label(),
            Self::Group(group) => group.label(),
        }
    }

    fn icon(self, tool: EditorTool) -> egui::ImageSource<'static> {
        match self {
            Self::Action(action) => action.icon(),
            Self::Group(group) => group.icon(tool),
        }
    }

    fn group(self) -> Option<QuickGroup> {
        match self {
            Self::Action(_) => None,
            Self::Group(group) => Some(group),
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
    chosen: Option<QuickTarget>,
    progress: f32,
    hover: [f32; QUICK_SLOTS.len()],
    /// The ring slot whose arc is shown. It stays set while the arc folds away.
    group: Option<usize>,
    group_open: bool,
    group_progress: f32,
    group_hover: [f32; QUICK_MENU_GROUP_CAPACITY],
}

impl QuickMenu {
    fn open_group(&self) -> Option<usize> {
        self.group.filter(|_| self.group_open)
    }

    fn group_actions(&self, brushes: usize) -> Vec<QuickAction> {
        self.group
            .map_or_else(Vec::new, |slot| group_actions(slot, brushes))
    }

    fn action(&self, target: QuickTarget, brushes: usize) -> Option<QuickAction> {
        match target {
            QuickTarget::Slot(slot) => match QUICK_SLOTS[slot] {
                QuickSlot::Action(action) => Some(action),
                QuickSlot::Group(_) => None,
            },
            QuickTarget::GroupAction(index) => self.group_actions(brushes).get(index).copied(),
        }
    }

    fn show_group(&mut self, group: Option<usize>) {
        match group {
            Some(slot) if self.group != Some(slot) => {
                self.group = Some(slot);
                self.group_open = true;
                self.group_progress = 0.0;
                self.group_hover = [0.0; QUICK_MENU_GROUP_CAPACITY];
            }
            Some(_) => self.group_open = true,
            None => self.group_open = false,
        }
    }
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
            hover: [0.0; QUICK_SLOTS.len()],
            group: None,
            group_open: false,
            group_progress: 0.0,
            group_hover: [0.0; QUICK_MENU_GROUP_CAPACITY],
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
        layers: &LayerSnapshot,
    ) {
        if self.quick_menu.is_none() {
            return;
        }
        let (brushes, selected_brush) = self.quick_brushes(tool.paint_tool());
        let Some(menu) = self.quick_menu.as_mut() else {
            return;
        };
        let context = QuickMenuContext {
            selected_brush,
            ..QuickMenuContext::new(menu_state, tool, layers)
        };
        let (pointer, primary_pressed, secondary_pressed, dt) = ctx.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_pressed(),
                input.pointer.secondary_pressed(),
                input.stable_dt.min(0.1),
            )
        });
        let target = if menu.open {
            let target = pointer.and_then(|pointer| {
                quick_menu_target_at(pointer - menu.center, menu.open_group(), brushes.len())
            });
            menu.show_group(group_after_hover(menu.open_group(), target));
            target
        } else {
            menu.chosen
        };
        let action = target.and_then(|target| menu.action(target, brushes.len()));
        let enabled_action = action.filter(|action| action.is_enabled(context));
        // Group slots stay hoverable; disabled actions do not highlight.
        let hovered = target.filter(|_| action.is_none() || enabled_action.is_some());

        if menu.open {
            let released_key = std::mem::take(&mut menu.key_released);
            if released_key {
                menu.held_by_key = false;
            }
            if let Some(action) = enabled_action.filter(|_| primary_pressed || released_key) {
                menu.open = false;
                menu.chosen = hovered;
                self.commands
                    .push(action.command(context.selected_layer, &brushes));
            } else if secondary_pressed || (primary_pressed && hovered.is_none()) {
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
        menu.group_progress = step_toward(
            menu.group_progress,
            f32::from(menu.group_open),
            dt,
            QUICK_MENU_GROUP_SECONDS,
        );
        animating |= menu.group_progress != f32::from(menu.group_open);
        if !menu.group_open && menu.group_progress == 0.0 {
            menu.group = None;
        }
        for (slot, hover) in menu.hover.iter_mut().enumerate() {
            let target = f32::from(hovered == Some(QuickTarget::Slot(slot)));
            *hover = step_toward(*hover, target, dt, QUICK_MENU_HOVER_SECONDS);
            animating |= *hover != target;
        }
        for (index, hover) in menu.group_hover.iter_mut().enumerate() {
            let target = f32::from(hovered == Some(QuickTarget::GroupAction(index)));
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
        let frosted = self.surface_style == SurfaceStyle::Frosted;
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
                paint_quick_menu(ui, menu, context, &brushes, frosted);
            });
    }

    /// The active paint tool's brushes, and the index of its selected brush.
    fn quick_brushes(&mut self, tool: Option<PaintTool>) -> (Vec<QuickBrush>, Option<usize>) {
        let Some(tool) = tool else {
            return (Vec::new(), None);
        };
        self.cache_next_missing_brush_preview();
        // shortcut: only the first brushes that fit the arc are listed; page the arc once
        // imported brush sets commonly outgrow it.
        let brushes: Vec<_> = self
            .brushes
            .iter()
            .take(QUICK_MENU_GROUP_CAPACITY)
            .map(|brush| QuickBrush {
                tool,
                id: brush.id.clone(),
                name: brush.name.clone(),
                texture: self.brush_preview_texture(&brush.id),
            })
            .collect();
        let selected = brushes
            .iter()
            .position(|brush| brush.id == self.tool_brushes[tool_index(tool)]);
        (brushes, selected)
    }
}

fn paint_quick_menu(
    ui: &egui::Ui,
    menu: &QuickMenu,
    context: QuickMenuContext,
    brushes: &[QuickBrush],
    frosted: bool,
) {
    let visuals = ui.visuals();
    let accent = visuals.selection.bg_fill;
    let on_accent = visuals.selection.stroke.color;
    let text_color = visuals.text_color();
    let border = egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);
    let progress = egui::emath::easing::cubic_out(menu.progress);
    let content_alpha = progress * progress;
    let resting_tint = |action: Option<QuickAction>| match action {
        Some(action) if !action.is_enabled(context) => {
            visuals.weak_text_color().gamma_multiply(0.5)
        }
        Some(QuickAction::Preset(index)) if context.selected_brush == Some(index) => accent,
        Some(action) if action.tool() == Some(context.tool) => accent,
        _ => text_color,
    };
    paint_quick_menu_surface(
        ui,
        menu.center,
        QUICK_MENU_RADIUS * progress,
        visuals.window_fill(),
        border,
    );

    for (slot, quick_slot) in QUICK_SLOTS.into_iter().enumerate() {
        let direction = quick_menu_direction(quick_menu_slot_angle(slot));
        let slot_center = menu.center + direction * QUICK_MENU_SLOT_RADIUS * progress;
        let hover = egui::emath::easing::cubic_out(menu.hover[slot]);
        let highlight_radius = QUICK_MENU_HIGHLIGHT_RADIUS * progress;
        if menu.group == Some(slot) {
            ui.painter().circle_filled(
                slot_center,
                highlight_radius,
                accent.gamma_multiply(0.3 * menu.group_progress * content_alpha),
            );
        }
        if hover > 0.0 {
            ui.painter().circle_filled(
                slot_center,
                highlight_radius * hover,
                accent.gamma_multiply(content_alpha),
            );
        }
        let tint = resting_tint(menu.action(QuickTarget::Slot(slot), brushes.len()))
            .lerp_to_gamma(on_accent, hover)
            .gamma_multiply(content_alpha);
        let icon_rect = egui::Rect::from_center_size(
            slot_center,
            egui::Vec2::splat(QUICK_MENU_ICON_SIZE * progress),
        );
        match (quick_slot, context.selected_brush) {
            (QuickSlot::Group(QuickGroup::Brush), Some(index)) => {
                let stamp_rect =
                    icon_rect.scale_from_center(QUICK_MENU_STAMP_SIZE / QUICK_MENU_ICON_SIZE);
                paint_stamp(ui, stamp_rect, brushes[index].texture, tint);
            }
            _ => egui::Image::new(quick_slot.icon(context.tool))
                .tint(tint)
                .alt_text(quick_slot.label())
                .paint_at(ui, icon_rect),
        }
    }

    let mut label = QUICK_SLOTS
        .iter()
        .zip(menu.hover)
        .map(|(slot, hover)| (slot.label(), hover))
        .max_by(|a, b| a.1.total_cmp(&b.1));

    if let Some(group_slot) = menu
        .group
        .filter(|&slot| !group_actions(slot, brushes.len()).is_empty())
    {
        let actions = menu.group_actions(brushes.len());
        let group_progress = egui::emath::easing::cubic_out(menu.group_progress);
        let scale = group_progress * progress;
        let group_alpha = content_alpha * group_progress * group_progress;
        // The band grows out of the disc's rim beside the group's slot while its ends spread apart.
        let ring_radius = QUICK_MENU_RADIUS * progress;
        let band_width = QUICK_MENU_BAND_WIDTH * scale;
        let radius = ring_radius + (QUICK_MENU_GROUP_RADIUS - QUICK_MENU_RADIUS) * scale;
        let slot_angle = quick_menu_slot_angle(group_slot);
        let angle_at = |index: usize| {
            slot_angle
                + (quick_menu_group_action_angle(group_slot, index, actions.len()) - slot_angle)
                    * group_progress
        };
        let end_padding = quick_menu_group_end_padding() * group_progress;
        // The frosted pass restyles the band's fill but not its separate outline.
        let band_border = if frosted {
            frosted_surface_edge()
        } else {
            border
        };
        paint_quick_menu_band(
            ui,
            QuickMenuBand::new(
                menu.center,
                ring_radius,
                band_width,
                QUICK_MENU_BAND_CORNER_RADIUS * scale,
                [
                    angle_at(0) - end_padding,
                    angle_at(actions.len() - 1) + end_padding,
                ],
            ),
            visuals.window_fill(),
            band_border,
        );
        for (index, &action) in actions.iter().enumerate() {
            let action_center = menu.center + quick_menu_direction(angle_at(index)) * radius;
            let hover = egui::emath::easing::cubic_out(menu.group_hover[index]);
            if hover > 0.0 {
                ui.painter().circle_filled(
                    action_center,
                    QUICK_MENU_HIGHLIGHT_RADIUS * scale * hover,
                    accent.gamma_multiply(group_alpha),
                );
            }
            let tint = resting_tint(Some(action))
                .lerp_to_gamma(on_accent, hover)
                .gamma_multiply(group_alpha);
            let icon_rect = egui::Rect::from_center_size(
                action_center,
                egui::Vec2::splat(QUICK_MENU_ICON_SIZE * scale),
            );
            let action_label = match action {
                QuickAction::Preset(index) => {
                    let brush = &brushes[index];
                    let stamp_rect =
                        icon_rect.scale_from_center(QUICK_MENU_STAMP_SIZE / QUICK_MENU_ICON_SIZE);
                    paint_stamp(ui, stamp_rect, brush.texture, tint);
                    brush.name.as_str()
                }
                action => {
                    egui::Image::new(action.icon())
                        .tint(tint)
                        .alt_text(action.label())
                        .paint_at(ui, icon_rect);
                    action.label()
                }
            };
            if label.is_none_or(|(_, strongest)| hover > strongest) {
                label = Some((action_label, hover));
            }
        }
    }

    if let Some((label, hover)) = label.filter(|(_, hover)| *hover > 0.0) {
        ui.painter().text(
            menu.center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(13.0),
            text_color.gamma_multiply(content_alpha * hover),
        );
    }
}

/// A fully rounded surface-colored rect, rather than a circle shape, keeps frosted surfaces
/// blurring the canvas behind the menu.
fn paint_quick_menu_surface(
    ui: &egui::Ui,
    center: egui::Pos2,
    radius: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    ui.painter().add(egui::epaint::RectShape::new(
        egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0)),
        egui::CornerRadius::same(radius.round() as u8),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    ));
}

/// An annular sector that grows out of the disc's rim, with rounded outer corners.
#[derive(Clone, Copy, Debug)]
struct QuickMenuBand {
    center: egui::Pos2,
    /// Where the band meets the disc.
    ring_radius: f32,
    inner_radius: f32,
    outer_radius: f32,
    corner_radius: f32,
    start: f32,
    end: f32,
}

impl QuickMenuBand {
    fn new(
        center: egui::Pos2,
        ring_radius: f32,
        width: f32,
        corner_radius: f32,
        [start, end]: [f32; 2],
    ) -> Self {
        let inner_radius = (ring_radius - QUICK_MENU_BAND_OVERLAP).max(0.0);
        // Small enough that both corners fit between the band's straight edges.
        let corner_radius = corner_radius
            .min(width / 2.0)
            .min(inner_radius * ((end - start) / 2.0).tan());
        Self {
            center,
            ring_radius,
            inner_radius,
            outer_radius: ring_radius + width,
            corner_radius,
            start,
            end,
        }
    }

    /// Points along the band's outside edge with outward normals: out along the start edge,
    /// around the outer arc, and back in along the end edge.
    fn edge(self) -> Vec<(egui::Pos2, egui::Vec2)> {
        let tangent = |angle: f32| egui::vec2(angle.cos(), angle.sin());
        let corner = self.corner_radius;
        // Distance from the menu center to each corner arc's center, along the straight edge.
        let reach = ((self.outer_radius - corner).powi(2) - corner.powi(2))
            .max(0.0)
            .sqrt();
        let corner_angle = corner.atan2(reach);
        let corner_points = |angle: f32, inward: f32| {
            let inward = tangent(angle) * inward;
            let corner_center = self.center + quick_menu_direction(angle) * reach + inward * corner;
            let from = (-inward).angle();
            let to = (corner_center - self.center).angle();
            let sweep = (to - from + PI).rem_euclid(TAU) - PI;
            (0..=QUICK_MENU_BAND_CORNER_STEPS).map(move |step| {
                let normal = egui::Vec2::angled(
                    from + sweep * step as f32 / QUICK_MENU_BAND_CORNER_STEPS as f32,
                );
                (corner_center + normal * corner, normal)
            })
        };
        let arc_start = self.start + corner_angle;
        let arc_end = self.end - corner_angle;
        let steps = quick_menu_band_steps(arc_start, arc_end);
        let mut edge = vec![(
            self.center + quick_menu_direction(self.start) * self.inner_radius,
            -tangent(self.start),
        )];
        edge.extend(corner_points(self.start, 1.0));
        edge.extend((1..steps).map(|step| {
            let direction =
                quick_menu_direction(egui::lerp(arc_start..=arc_end, step as f32 / steps as f32));
            (self.center + direction * self.outer_radius, direction)
        }));
        let end_corner: Vec<_> = corner_points(self.end, -1.0).collect();
        edge.extend(end_corner.into_iter().rev());
        edge.push((
            self.center + quick_menu_direction(self.end) * self.inner_radius,
            tangent(self.end),
        ));
        edge
    }
}

/// The band is a surface-colored mesh, which the frosted pass blurs like the disc. Its outline
/// leaves the joint with the disc open so the two read as one surface.
fn paint_quick_menu_band(
    ui: &egui::Ui,
    band: QuickMenuBand,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    let feather = 1.0 / ui.ctx().pixels_per_point();
    if band.outer_radius - band.ring_radius <= feather {
        return;
    }
    ui.painter().add(quick_menu_band_mesh(band, fill, feather));
    if stroke.width > 0.0 {
        ui.painter().add(egui::epaint::PathShape::line(
            quick_menu_band_outline(band, stroke.width),
            stroke,
        ));
    }
}

/// Builds the band with a one-pixel transparent fringe for anti-aliasing, since egui only
/// anti-aliases convex fills. Each edge point pairs with the inner arc point in its direction.
fn quick_menu_band_mesh(band: QuickMenuBand, fill: egui::Color32, feather: f32) -> egui::Mesh {
    let transparent = egui::Color32::TRANSPARENT;
    let mut mesh = egui::Mesh::default();
    for (index, (point, normal)) in band.edge().into_iter().enumerate() {
        let radial = (point - band.center).normalized();
        mesh.colored_vertex(
            band.center + radial * (band.inner_radius - feather / 2.0),
            transparent,
        );
        mesh.colored_vertex(
            band.center + radial * (band.inner_radius + feather / 2.0),
            fill,
        );
        mesh.colored_vertex(point - normal * (feather / 2.0), fill);
        mesh.colored_vertex(point + normal * (feather / 2.0), transparent);
        if index > 0 {
            let previous = 4 * (index as u32 - 1);
            let current = previous + 4;
            for lane in 0..3 {
                mesh.add_triangle(previous + lane, previous + lane + 1, current + lane);
                mesh.add_triangle(previous + lane + 1, current + lane + 1, current + lane);
            }
        }
    }
    mesh
}

/// Insets the edge by half the stroke, and starts and ends it on the disc's border.
fn quick_menu_band_outline(band: QuickMenuBand, stroke_width: f32) -> Vec<egui::Pos2> {
    let mut edge = band.edge();
    let last = edge.len() - 1;
    for index in [0, last] {
        let radial = (edge[index].0 - band.center).normalized();
        edge[index].0 = band.center + radial * (band.ring_radius - stroke_width / 2.0);
    }
    edge.into_iter()
        .map(|(point, normal)| point - normal * (stroke_width / 2.0))
        .collect()
}

fn quick_menu_band_steps(start: f32, end: f32) -> usize {
    ((end - start).abs() / QUICK_MENU_BAND_SEGMENT)
        .ceil()
        .max(1.0) as usize
}

/// Clockwise angle from the top, in radians.
fn quick_menu_slot_angle(slot: usize) -> f32 {
    slot as f32 * TAU / QUICK_SLOTS.len() as f32
}

/// Spreads a group's actions evenly around its ring slot's angle.
fn quick_menu_group_action_angle(slot: usize, index: usize, count: usize) -> f32 {
    quick_menu_slot_angle(slot)
        + (index as f32 - (count - 1) as f32 / 2.0) * QUICK_MENU_GROUP_SPACING
}

/// Angle the band extends past its first and last actions, so their highlights keep the edge
/// padding from the band's straight ends.
fn quick_menu_group_end_padding() -> f32 {
    ((QUICK_MENU_HIGHLIGHT_RADIUS + QUICK_MENU_EDGE_PADDING) / QUICK_MENU_GROUP_RADIUS).asin()
}

/// Unit vector for a clockwise angle from the top, in egui's y-down coordinates.
fn quick_menu_direction(angle: f32) -> egui::Vec2 {
    egui::vec2(angle.sin(), -angle.cos())
}

fn quick_menu_angle_of(offset: egui::Vec2) -> f32 {
    offset.x.atan2(-offset.y).rem_euclid(TAU)
}

/// Picks a slot by direction alone so wide wedges are easy to hit with a pen. Points in the
/// center dead zone or far outside the ring select nothing.
fn quick_menu_slot_at(offset: egui::Vec2) -> Option<usize> {
    let distance = offset.length();
    if !(QUICK_MENU_DEAD_ZONE..=QUICK_MENU_REACH).contains(&distance) {
        return None;
    }
    let slot_angle = TAU / QUICK_SLOTS.len() as f32;
    Some(
        ((quick_menu_angle_of(offset) + slot_angle / 2.0) / slot_angle) as usize
            % QUICK_SLOTS.len(),
    )
}

/// Outside the disc, the open group's arc takes precedence over the ring. Between the icons and
/// the rim, the group keeps the whole angle range its arc covers, so a diagonal path toward an
/// edge action does not cut through a neighboring slot and switch groups. Closer in, where moves
/// between neighboring icons pass, it keeps only half the extra range so switching stays quick.
fn quick_menu_target_at(
    offset: egui::Vec2,
    open_group: Option<usize>,
    brushes: usize,
) -> Option<QuickTarget> {
    if let Some(slot) = open_group {
        let count = group_actions(slot, brushes).len();
        let distance = offset.length();
        let half_span = count.saturating_sub(1) as f32 / 2.0 * QUICK_MENU_GROUP_SPACING;
        let delta =
            (quick_menu_angle_of(offset) - quick_menu_slot_angle(slot) + PI).rem_euclid(TAU) - PI;
        if count > 0
            && distance > QUICK_MENU_RADIUS
            && distance <= QUICK_MENU_GROUP_REACH
            && delta.abs() <= half_span + quick_menu_group_end_padding()
        {
            let index = ((delta + half_span) / QUICK_MENU_GROUP_SPACING)
                .round()
                .clamp(0.0, (count - 1) as f32) as usize;
            return Some(QuickTarget::GroupAction(index));
        }
        let wedge = TAU / QUICK_SLOTS.len() as f32 / 2.0;
        let hold = if distance > QUICK_MENU_SLOT_RADIUS {
            half_span
        } else {
            (wedge + half_span) / 2.0
        };
        if (QUICK_MENU_DEAD_ZONE..=QUICK_MENU_RADIUS).contains(&distance) && delta.abs() <= hold {
            return Some(QuickTarget::Slot(slot));
        }
    }
    quick_menu_slot_at(offset).map(QuickTarget::Slot)
}

fn group_actions(slot: usize, brushes: usize) -> Vec<QuickAction> {
    QUICK_SLOTS[slot]
        .group()
        .map_or_else(Vec::new, |group| group.actions(brushes))
}

/// Hovering a ring slot opens its group or closes the open one. Elsewhere the open group stays,
/// so overshooting the arc or returning to the center does not make it flicker.
fn group_after_hover(group: Option<usize>, target: Option<QuickTarget>) -> Option<usize> {
    match target {
        Some(QuickTarget::Slot(slot)) => QUICK_SLOTS[slot].group().map(|_| slot),
        _ => group,
    }
}

/// Keeps the ring and any group arc inside the window, centering it when the window is too small.
fn quick_menu_center(pointer: egui::Pos2, screen: egui::Rect) -> egui::Pos2 {
    let inset = QUICK_MENU_RADIUS + QUICK_MENU_BAND_WIDTH + QUICK_MENU_SCREEN_MARGIN;
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

    const TOOL_SLOT: usize = 0;
    const BRUSH_SLOT: usize = 1;
    const LAYER_SLOT: usize = 3;
    const CANVAS_SLOT: usize = 4;
    const BRUSHES: usize = QUICK_MENU_GROUP_CAPACITY;

    fn offset_at(degrees: f32, distance: f32) -> egui::Vec2 {
        quick_menu_direction(degrees.to_radians()) * distance
    }

    fn group_action_offset(slot: usize, index: usize) -> egui::Vec2 {
        let count = QUICK_SLOTS[slot].group().unwrap().actions(BRUSHES).len();
        quick_menu_direction(quick_menu_group_action_angle(slot, index, count))
            * QUICK_MENU_GROUP_RADIUS
    }

    fn layer(id: u64, visible: bool) -> LayerInfo {
        LayerInfo {
            id: LayerId(id),
            name: format!("Layer {id}"),
            visible,
            opacity: 255,
            clipped: false,
            blend_mode: BlendMode::default(),
        }
    }

    fn context(
        menu: ApplicationMenuState,
        layers: Vec<LayerInfo>,
        selection: u64,
    ) -> QuickMenuContext {
        let layers = LayerSnapshot {
            layers,
            selection: LayerId(selection),
            background_color: [1.0; 4],
        };
        QuickMenuContext::new(menu, EditorTool::default(), &layers)
    }

    #[test]
    fn quick_menu_slots_run_clockwise_from_the_top() {
        for slot in 0..QUICK_SLOTS.len() {
            let offset = quick_menu_direction(quick_menu_slot_angle(slot)) * QUICK_MENU_SLOT_RADIUS;
            assert_eq!(quick_menu_slot_at(offset), Some(slot));
        }
        assert_eq!(quick_menu_slot_at(egui::vec2(0.0, -60.0)), Some(0));
    }

    #[test]
    fn quick_menu_slot_boundaries_wrap_around_the_top() {
        assert_eq!(quick_menu_slot_at(offset_at(25.0, 60.0)), Some(0));
        assert_eq!(quick_menu_slot_at(offset_at(26.0, 60.0)), Some(1));
        assert_eq!(quick_menu_slot_at(offset_at(334.0, 60.0)), Some(6));
        assert_eq!(quick_menu_slot_at(offset_at(335.0, 60.0)), Some(0));
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
            quick_menu_slot_at(offset_at(60.0, QUICK_MENU_REACH)),
            Some(1)
        );
    }

    #[test]
    fn groups_fit_their_arc() {
        for slot in QUICK_SLOTS {
            if let Some(group) = slot.group() {
                assert!((1..=QUICK_MENU_GROUP_CAPACITY).contains(&group.actions(BRUSHES).len()));
            }
        }
    }

    #[test]
    fn group_actions_run_clockwise_along_the_arc() {
        for slot in [TOOL_SLOT, BRUSH_SLOT, LAYER_SLOT, CANVAS_SLOT] {
            let count = QUICK_SLOTS[slot].group().unwrap().actions(BRUSHES).len();
            for index in 0..count {
                assert_eq!(
                    quick_menu_target_at(group_action_offset(slot, index), Some(slot), BRUSHES),
                    Some(QuickTarget::GroupAction(index))
                );
            }
            let first = quick_menu_angle_of(group_action_offset(slot, 0));
            let last = quick_menu_angle_of(group_action_offset(slot, count - 1));
            // The tool arc straddles the top, where angles wrap.
            assert!((last - first).rem_euclid(TAU) < PI);
        }
    }

    #[test]
    fn pointer_outside_the_open_arc_returns_to_ring_slots() {
        assert_eq!(
            quick_menu_target_at(offset_at(45.0, 130.0), Some(LAYER_SLOT), BRUSHES),
            Some(QuickTarget::Slot(1))
        );
        assert_eq!(
            quick_menu_target_at(
                offset_at(135.0, QUICK_MENU_GROUP_REACH + 1.0),
                Some(LAYER_SLOT),
                BRUSHES
            ),
            None
        );
        assert_eq!(
            quick_menu_target_at(offset_at(154.0, QUICK_MENU_GROUP_RADIUS), None, BRUSHES),
            Some(QuickTarget::Slot(LAYER_SLOT))
        );
        assert_eq!(
            quick_menu_target_at(egui::Vec2::ZERO, Some(LAYER_SLOT), BRUSHES),
            None
        );
    }

    #[test]
    fn diagonal_path_to_an_edge_action_keeps_the_group_open() {
        let start =
            quick_menu_direction(quick_menu_slot_angle(LAYER_SLOT)) * QUICK_MENU_SLOT_RADIUS;
        let last = QuickGroup::Layer.actions(BRUSHES).len() - 1;
        let end = group_action_offset(LAYER_SLOT, last);
        let mut group = Some(LAYER_SLOT);
        let mut crossed_neighbor = false;
        for step in 0..=20 {
            let offset = start + (end - start) * (step as f32 / 20.0);
            crossed_neighbor |= quick_menu_slot_at(offset) == Some(CANVAS_SLOT);
            group = group_after_hover(group, quick_menu_target_at(offset, group, BRUSHES));
            assert_eq!(group, Some(LAYER_SLOT));
        }
        assert!(crossed_neighbor);
        assert_eq!(
            quick_menu_target_at(end, group, BRUSHES),
            Some(QuickTarget::GroupAction(last))
        );
    }

    #[test]
    fn moving_between_neighboring_icons_switches_groups_before_reaching_the_next_icon() {
        let layer =
            quick_menu_direction(quick_menu_slot_angle(LAYER_SLOT)) * QUICK_MENU_SLOT_RADIUS;
        let canvas =
            quick_menu_direction(quick_menu_slot_angle(CANVAS_SLOT)) * QUICK_MENU_SLOT_RADIUS;
        let mut group = Some(LAYER_SLOT);
        for step in 0..=20 {
            let offset = layer + (canvas - layer) * (step as f32 / 20.0);
            group = group_after_hover(group, quick_menu_target_at(offset, group, BRUSHES));
            if step <= 10 {
                assert_eq!(group, Some(LAYER_SLOT));
            } else if step >= 15 {
                assert_eq!(group, Some(CANVAS_SLOT));
            }
        }
    }

    #[test]
    fn hovering_a_direct_slot_closes_the_open_group() {
        assert_eq!(
            group_after_hover(None, Some(QuickTarget::Slot(LAYER_SLOT))),
            Some(LAYER_SLOT)
        );
        assert_eq!(
            group_after_hover(Some(LAYER_SLOT), Some(QuickTarget::Slot(CANVAS_SLOT))),
            Some(CANVAS_SLOT)
        );
        assert_eq!(
            group_after_hover(Some(LAYER_SLOT), Some(QuickTarget::Slot(2))),
            None
        );
        assert_eq!(group_after_hover(Some(LAYER_SLOT), None), Some(LAYER_SLOT));
        assert_eq!(
            group_after_hover(Some(LAYER_SLOT), Some(QuickTarget::GroupAction(2))),
            Some(LAYER_SLOT)
        );
    }

    fn layer_band() -> QuickMenuBand {
        let actions = QuickGroup::Layer.actions(BRUSHES).len();
        QuickMenuBand::new(
            egui::Pos2::ZERO,
            QUICK_MENU_RADIUS,
            QUICK_MENU_BAND_WIDTH,
            QUICK_MENU_BAND_CORNER_RADIUS,
            [
                quick_menu_group_action_angle(LAYER_SLOT, 0, actions)
                    - quick_menu_group_end_padding(),
                quick_menu_group_action_angle(LAYER_SLOT, actions - 1, actions)
                    + quick_menu_group_end_padding(),
            ],
        )
    }

    #[test]
    fn group_band_pads_action_highlights_like_the_ring() {
        let ring_padding = QUICK_MENU_RADIUS - QUICK_MENU_SLOT_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
        let band = layer_band();
        assert!(quick_menu_band_mesh(band, egui::Color32::WHITE, 1.0).is_valid());
        let edge = band.edge();
        let actions = QuickGroup::Layer.actions(BRUSHES).len();
        for index in 0..actions {
            let action = egui::Pos2::ZERO + group_action_offset(LAYER_SLOT, index);
            for (point, _) in &edge {
                assert!(
                    point.distance(action) >= QUICK_MENU_HIGHLIGHT_RADIUS + ring_padding - 0.01
                );
            }
            let outer_padding =
                band.outer_radius - QUICK_MENU_GROUP_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
            assert!((outer_padding - ring_padding).abs() < 0.01);
        }
        let ring_highlight_edge = QUICK_MENU_SLOT_RADIUS + QUICK_MENU_HIGHLIGHT_RADIUS;
        let band_highlight_edge = QUICK_MENU_GROUP_RADIUS - QUICK_MENU_HIGHLIGHT_RADIUS;
        assert!((band_highlight_edge - ring_highlight_edge - ring_padding).abs() < 0.01);
    }

    #[test]
    fn group_band_outline_starts_and_ends_on_the_disc_border() {
        let band = layer_band();
        let outline = quick_menu_band_outline(band, 1.0);
        let [first, last] = [outline[0], outline[outline.len() - 1]];
        for point in [first, last] {
            assert!((point.to_vec2().length() - (QUICK_MENU_RADIUS - 0.5)).abs() < 0.6);
        }
        for point in &outline[1..outline.len() - 1] {
            assert!(point.to_vec2().length() > QUICK_MENU_RADIUS);
        }
        assert!(band.inner_radius < QUICK_MENU_RADIUS - 0.5);
    }

    #[test]
    fn group_band_corners_shrink_to_fit_a_narrow_band() {
        let band = QuickMenuBand::new(egui::Pos2::ZERO, 100.0, 4.0, 10.0, [0.0, 0.01]);
        assert!(band.corner_radius <= 2.0);
        assert!(band.corner_radius <= band.inner_radius * 0.005_f32.tan());
        assert!(quick_menu_band_mesh(band, egui::Color32::WHITE, 1.0).is_valid());
    }

    #[test]
    fn quick_menu_stays_inside_the_window() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let inset = QUICK_MENU_RADIUS + QUICK_MENU_BAND_WIDTH + QUICK_MENU_SCREEN_MARGIN;

        assert_eq!(
            quick_menu_center(egui::pos2(400.0, 300.0), screen),
            egui::pos2(400.0, 300.0)
        );
        assert_eq!(
            quick_menu_center(egui::pos2(5.0, 590.0), screen),
            egui::pos2(inset, 600.0 - inset)
        );

        let small = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 200.0));
        assert_eq!(
            quick_menu_center(egui::pos2(0.0, 0.0), small),
            small.center()
        );
    }

    #[test]
    fn quick_actions_dispatch_existing_editor_commands() {
        let selected = LayerId(7);
        let editor = |action: QuickAction| match action.command(selected, &[]) {
            AppCommand::Editor(command) => command,
            command => panic!("{action:?} dispatched {command:?}"),
        };
        let ring = QUICK_SLOTS.map(|slot| match slot {
            QuickSlot::Action(action) => Some(editor(action)),
            QuickSlot::Group(_) => None,
        });
        assert_eq!(
            ring,
            [
                None,
                None,
                Some(EditorCommand::Redo),
                None,
                None,
                Some(EditorCommand::SelectTool(EditorTool::Select)),
                Some(EditorCommand::Undo),
            ]
        );
        let tool: Vec<_> = QuickGroup::Tool
            .actions(BRUSHES)
            .iter()
            .map(|&action| editor(action))
            .collect();
        assert_eq!(
            tool,
            [
                EditorCommand::SelectTool(PaintTool::Brush.into()),
                EditorCommand::SelectTool(PaintTool::Eraser.into()),
                EditorCommand::SelectTool(PaintTool::Smudge.into()),
            ]
        );
        let brushes = [QuickBrush {
            tool: PaintTool::Eraser,
            id: "soft".to_owned(),
            name: "Soft".to_owned(),
            texture: None,
        }];
        assert_eq!(
            QuickGroup::Brush.actions(brushes.len()),
            [QuickAction::Preset(0)]
        );
        assert_eq!(
            QuickAction::Preset(0).command(selected, &brushes),
            AppCommand::Settings(SettingsCommand::SwitchBrush {
                tool: PaintTool::Eraser,
                id: "soft".to_owned(),
            })
        );
        let layer: Vec<_> = QuickGroup::Layer
            .actions(BRUSHES)
            .iter()
            .map(|&action| editor(action))
            .collect();
        assert_eq!(
            layer,
            [
                EditorCommand::AddLayer,
                EditorCommand::DuplicateSelectedLayer,
                EditorCommand::MergeLayerDown(selected),
                EditorCommand::ClearLayer,
                EditorCommand::DeleteSelectedLayer,
            ]
        );
        let canvas: Vec<_> = QuickGroup::Canvas
            .actions(BRUSHES)
            .iter()
            .map(|&action| editor(action))
            .collect();
        assert_eq!(
            canvas,
            [
                EditorCommand::RotateCanvasRight,
                EditorCommand::ToggleCanvasFlipHorizontal,
                EditorCommand::ResetCanvasRotation,
                EditorCommand::ToggleCanvasFlipVertical,
                EditorCommand::RotateCanvasLeft,
            ]
        );
    }

    #[test]
    fn unavailable_actions_are_disabled() {
        let menu = ApplicationMenuState::new(true, false, false, true, false);
        let bottom = context(menu, vec![layer(1, true), layer(2, true)], 1);
        assert!(!QuickAction::Undo.is_enabled(bottom));
        assert!(QuickAction::Redo.is_enabled(bottom));
        assert!(!QuickAction::MergeDown.is_enabled(bottom));
        assert!(QuickAction::DeleteLayer.is_enabled(bottom));
        assert!(QuickAction::RotateLeft.is_enabled(bottom));

        let top = context(menu, vec![layer(1, true), layer(2, true)], 2);
        assert!(QuickAction::MergeDown.is_enabled(top));
        let hidden = context(menu, vec![layer(1, true), layer(2, false)], 2);
        assert!(!QuickAction::MergeDown.is_enabled(hidden));

        let cropping = ApplicationMenuState::new(true, true, true, true, false);
        let cropping = context(cropping, vec![layer(1, true)], 1);
        assert!(!QuickAction::FlipHorizontal.is_enabled(cropping));
        assert!(QuickAction::ClearLayer.is_enabled(cropping));
    }

    #[test]
    fn animation_steps_reach_their_target_without_overshooting() {
        assert_eq!(step_toward(0.0, 1.0, 0.06, 0.12), 0.5);
        assert_eq!(step_toward(0.9, 1.0, 0.06, 0.12), 1.0);
        assert_eq!(step_toward(0.1, 0.0, 0.06, 0.09), 0.0);
        assert_eq!(step_toward(1.0, 1.0, 0.06, 0.12), 1.0);
    }
}
