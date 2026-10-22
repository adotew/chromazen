use winit::window::Window;

pub(crate) const WINDOW_CONTROLS_MARGIN: f32 = 12.0;

#[cfg(not(target_os = "macos"))]
pub(crate) fn position_window_controls(_window: &Window) {}

#[cfg(target_os = "macos")]
pub(crate) fn position_window_controls(window: &Window) {
    use objc2::{MainThreadMarker, rc::Retained};
    use objc2_app_kit::{NSView, NSWindowButton};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Some(_) = MainThreadMarker::new() else {
        return;
    };
    if window.fullscreen().is_some() {
        return;
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // The handle belongs to this live winit window; AppKit access stays on the main thread.
    let Some(view) = (unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }) else {
        return;
    };
    let Some(ns_window) = view.window() else {
        return;
    };
    let Some(close) = ns_window.standardWindowButton(NSWindowButton::CloseButton) else {
        return;
    };
    // Grow the native titlebar container too, so the lowered buttons remain clickable.
    let Some(container) = (unsafe { close.superview().and_then(|parent| parent.superview()) })
    else {
        return;
    };
    let mut frame = container.frame();
    let height = 32.0 + f64::from(WINDOW_CONTROLS_MARGIN);
    frame.origin.x = f64::from(WINDOW_CONTROLS_MARGIN);
    frame.origin.y += frame.size.height - height;
    frame.size.width = ns_window.frame().size.width - f64::from(WINDOW_CONTROLS_MARGIN);
    frame.size.height = height;
    if container.frame() != frame {
        container.setFrame(frame);
    }

    for kind in [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ] {
        let Some(button) = ns_window.standardWindowButton(kind) else {
            continue;
        };
        let Some(parent) = (unsafe { button.superview() }) else {
            continue;
        };
        let frame = button.frame();
        let mut center = view.bounds().origin;
        center.y += controls_center_y(
            view.bounds().size.height,
            view.isFlipped(),
            WINDOW_CONTROLS_MARGIN,
        );
        let center = parent.convertPoint_fromView(center, Some(&view));
        let mut origin = frame.origin;
        origin.y = center.y - frame.size.height / 2.0;
        if frame.origin != origin {
            button.setFrameOrigin(origin);
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn controls_center_y(height: f64, flipped: bool, top_space: f32) -> f64 {
    let inset = f64::from(top_space) + 16.0;
    if flipped { inset } else { height - inset }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_center_is_inset_from_the_top_in_both_coordinate_systems() {
        assert_eq!(controls_center_y(900.0, true, WINDOW_CONTROLS_MARGIN), 28.0);
        assert_eq!(
            controls_center_y(900.0, false, WINDOW_CONTROLS_MARGIN),
            872.0
        );
        assert_eq!(controls_center_y(900.0, true, 0.0), 16.0);
        assert_eq!(controls_center_y(900.0, false, 0.0), 884.0);
    }
}
