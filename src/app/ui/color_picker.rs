use egui::{Color32, Response, Sense, Ui, ecolor::Hsva, ecolor::HsvaGamma, epaint::Mesh};

const GRADIENT_STEPS: u32 = 36;
const SLIDER_THUMB_SHADOW_OFFSET: f32 = 1.5;
const SLIDER_THUMB_SHADOW_ALPHA: u8 = 48;

#[derive(Clone, Copy)]
struct PickerState {
    color: Color32,
    hsva: Hsva,
}

pub(super) fn show(ui: &mut Ui, color: &mut Color32) -> bool {
    let state_id = ui.id().with("chromazen color picker state");
    let mut hsva = ui.ctx().data_mut(|data| {
        data.get_temp::<PickerState>(state_id)
            .filter(|state| state.color == *color)
            .map_or_else(|| Hsva::from(*color), |state| state.hsva)
    });
    let before = hsva;
    let mut hsvag = HsvaGamma::from(hsva);
    hsvag.a = 1.0;

    let width = ui.available_width();
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width;

        let opaque = HsvaGamma { a: 1.0, ..hsvag };
        color_slider_2d(ui, &mut hsvag.s, &mut hsvag.v, |s, v| {
            HsvaGamma { s, v, ..opaque }.into()
        });
        color_slider_1d(ui, &mut hsvag.h, |h| {
            HsvaGamma {
                h,
                s: 1.0,
                v: 1.0,
                a: 1.0,
            }
            .into()
        })
        .on_hover_text("Hue");

        let saturation_base = HsvaGamma { a: 1.0, ..hsvag };
        color_slider_1d(ui, &mut hsvag.s, |s| {
            HsvaGamma {
                s,
                ..saturation_base
            }
            .into()
        })
        .on_hover_text("Saturation");

        color_slider_1d(ui, &mut hsvag.v, |brightness| {
            HsvaGamma {
                h: 0.0,
                s: 0.0,
                v: brightness,
                a: 1.0,
            }
            .into()
        })
        .on_hover_text("Brightness");
    });

    hsva = Hsva::from(hsvag);
    *color = Color32::from(hsva);
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            state_id,
            PickerState {
                color: *color,
                hsva,
            },
        );
    });
    hsva != before
}

fn color_slider_1d(ui: &mut Ui, value: &mut f32, color_at: impl Fn(f32) -> Color32) -> Response {
    let desired_size = egui::vec2(ui.spacing().slider_width, ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_at_least(desired_size, Sense::click_and_drag());
    let thumb_radius = rect.height() * 0.32;
    let track_rect = egui::Rect::from_center_size(
        rect.center(),
        egui::vec2(
            (rect.width() - 2.0 * thumb_radius).max(0.0),
            6.0_f32.min(rect.height()),
        ),
    );

    if let Some(pointer) = response.interact_pointer_pos() {
        *value = egui::remap_clamp(pointer.x, track_rect.x_range(), 0.0..=1.0);
    }

    if ui.is_rect_visible(rect) {
        let mut mesh = Mesh::default();
        for index in 0..=GRADIENT_STEPS {
            let t = index as f32 / GRADIENT_STEPS as f32;
            let x = egui::lerp(track_rect.x_range(), t);
            let color = color_at(t);
            mesh.colored_vertex(egui::pos2(x, track_rect.top()), color);
            mesh.colored_vertex(egui::pos2(x, track_rect.bottom()), color);
            if index < GRADIENT_STEPS {
                mesh.add_triangle(2 * index, 2 * index + 1, 2 * index + 2);
                mesh.add_triangle(2 * index + 1, 2 * index + 2, 2 * index + 3);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        let track_radius = track_rect.height() / 2.0;
        ui.painter()
            .circle_filled(track_rect.left_center(), track_radius, color_at(0.0));
        ui.painter()
            .circle_filled(track_rect.right_center(), track_radius, color_at(1.0));

        let x = egui::lerp(track_rect.x_range(), *value);
        let picked = color_at(*value);
        let thumb_center = egui::pos2(x, track_rect.center().y);
        ui.painter().circle_filled(
            thumb_center + egui::vec2(0.0, SLIDER_THUMB_SHADOW_OFFSET),
            thumb_radius,
            Color32::from_black_alpha(SLIDER_THUMB_SHADOW_ALPHA),
        );
        ui.painter()
            .circle_filled(thumb_center, thumb_radius, picked);
    }

    response
}

fn color_slider_2d(
    ui: &mut Ui,
    x_value: &mut f32,
    y_value: &mut f32,
    color_at: impl Fn(f32, f32) -> Color32,
) -> Response {
    let desired_size = egui::Vec2::splat(ui.spacing().slider_width);
    let (rect, response) = ui.allocate_at_least(desired_size, Sense::click_and_drag());

    if let Some(pointer) = response.interact_pointer_pos() {
        *x_value = egui::remap_clamp(pointer.x, rect.x_range(), 0.0..=1.0);
        *y_value = egui::remap_clamp(pointer.y, rect.bottom()..=rect.top(), 0.0..=1.0);
    }

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let mut mesh = Mesh::default();
        for y_index in 0..=GRADIENT_STEPS {
            let y_t = y_index as f32 / GRADIENT_STEPS as f32;
            let y = egui::lerp(rect.bottom()..=rect.top(), y_t);
            for x_index in 0..=GRADIENT_STEPS {
                let x_t = x_index as f32 / GRADIENT_STEPS as f32;
                let x = egui::lerp(rect.x_range(), x_t);
                mesh.colored_vertex(egui::pos2(x, y), color_at(x_t, y_t));

                if x_index < GRADIENT_STEPS && y_index < GRADIENT_STEPS {
                    let row = GRADIENT_STEPS + 1;
                    let top_left = y_index * row + x_index;
                    mesh.add_triangle(top_left, top_left + 1, top_left + row);
                    mesh.add_triangle(top_left + 1, top_left + row, top_left + row + 1);
                }
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter()
            .rect_stroke(rect, 0.0, visuals.bg_stroke, egui::StrokeKind::Inside);

        let center = egui::pos2(
            egui::lerp(rect.x_range(), *x_value),
            egui::lerp(rect.bottom()..=rect.top(), *y_value),
        );
        let picked = color_at(*x_value, *y_value);
        ui.painter().circle(
            center,
            rect.width() / 12.0,
            picked,
            egui::Stroke::new(visuals.fg_stroke.width, contrast_color(picked)),
        );
    }

    response
}

fn contrast_color(color: Color32) -> Color32 {
    if egui::Rgba::from(color).intensity() < 0.5 {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}
