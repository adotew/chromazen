use std::sync::{Arc, Mutex};

#[cfg(not(target_os = "macos"))]
use winit::window::Window;

#[derive(Clone, Debug, Default)]
pub struct PressureStateHandle(Arc<Mutex<PressureState>>);

#[derive(Debug)]
struct PressureState {
    pressure: f32,
    pen_active: bool,
    pen_in_proximity: bool,
}

impl Default for PressureState {
    fn default() -> Self {
        Self {
            pressure: 1.0,
            pen_active: false,
            pen_in_proximity: false,
        }
    }
}

impl PressureState {
    fn brush_pressure(&self) -> f32 {
        if self.pen_active {
            self.pressure
        } else if self.pen_in_proximity {
            0.0
        } else {
            1.0
        }
    }
}

impl PressureStateHandle {
    pub fn brush_pressure(&self) -> f32 {
        self.0
            .lock()
            .expect("pressure state poisoned")
            .brush_pressure()
    }

    fn note_pen_pressure(&self, pressure: f32, active: bool, is_pen_device: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        state.pressure = pressure.clamp(0.0, 1.0);
        state.pen_active = active;
        state.pen_in_proximity |= is_pen_device;
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }

    fn end_pen_contact(&self, is_pen_device: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        state.pressure = 0.0;
        state.pen_active = false;
        state.pen_in_proximity |= is_pen_device;
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }

    fn set_pen_proximity(&self, in_proximity: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        state.pressure = 0.0;
        state.pen_active = false;
        state.pen_in_proximity = in_proximity;
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }

    fn clear_pen(&self) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        *state = PressureState::default();
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MacosPressureMonitor;

#[cfg(not(target_os = "macos"))]
impl MacosPressureMonitor {
    pub fn install(
        _window: Arc<Window>,
        _pressure_state: PressureStateHandle,
    ) -> Result<Option<Self>, String> {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::{ptr::NonNull, sync::Arc};

    use block2::{DynBlock, RcBlock};
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSPointingDeviceType, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    use super::PressureStateHandle;

    pub struct MacosPressureMonitor {
        monitor: Retained<AnyObject>,
        _handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>,
    }

    impl MacosPressureMonitor {
        pub fn install(
            window: Arc<Window>,
            pressure_state: PressureStateHandle,
        ) -> Result<Option<Self>, String> {
            let _mtm = MainThreadMarker::new().ok_or("AppKit access requires the main thread")?;
            let window_handle = window
                .window_handle()
                .map_err(|err| format!("failed to get window handle: {err}"))?;
            let RawWindowHandle::AppKit(handle) = window_handle.as_raw() else {
                return Err("expected an AppKit window handle on macOS".into());
            };

            let ns_view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
                .ok_or("failed to retain NSView from window handle")?;
            let ns_window = ns_view
                .window()
                .ok_or("NSView is not installed in an NSWindow")?;

            let handler = RcBlock::new(move |event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
                let Some(mtm) = MainThreadMarker::new() else {
                    return event_ptr.as_ptr();
                };

                let event = unsafe { event_ptr.as_ref() };
                let event_type = event.r#type();
                if let Some(event_window) = event.window(mtm) {
                    if !std::ptr::eq(&*event_window, &*ns_window) {
                        return event_ptr.as_ptr();
                    }
                } else if !matches!(event_type, NSEventType::TabletProximity) {
                    return event_ptr.as_ptr();
                }
                let is_pen_device = matches!(
                    event.pointingDeviceType(),
                    NSPointingDeviceType::Pen | NSPointingDeviceType::Eraser
                );
                let pressure = event.pressure();
                let has_meaningful_pressure = pressure > 0.0;
                let should_use_pressure = is_pen_device || has_meaningful_pressure;

                let changed = match event_type {
                    NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged => {
                        if should_use_pressure {
                            pressure_state.note_pen_pressure(pressure, true, is_pen_device)
                        } else {
                            pressure_state.clear_pen()
                        }
                    }
                    NSEventType::LeftMouseUp | NSEventType::MouseCancelled => {
                        pressure_state.end_pen_contact(is_pen_device)
                    }
                    NSEventType::TabletPoint | NSEventType::Pressure if should_use_pressure => {
                        pressure_state.note_pen_pressure(
                            pressure,
                            has_meaningful_pressure,
                            is_pen_device,
                        )
                    }
                    NSEventType::TabletProximity => {
                        pressure_state.set_pen_proximity(event.isEnteringProximity())
                    }
                    _ => false,
                };

                if changed {
                    window.request_redraw();
                }

                event_ptr.as_ptr()
            });
            let handler_ref: &DynBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> = &handler;

            let mask = NSEventMask::LeftMouseDown
                | NSEventMask::LeftMouseDragged
                | NSEventMask::LeftMouseUp
                | NSEventMask::Pressure
                | NSEventMask::TabletPoint
                | NSEventMask::TabletProximity
                | NSEventMask::MouseCancelled;

            let monitor =
                unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, handler_ref) }
                    .ok_or("failed to install AppKit event monitor")?;

            Ok(Some(Self {
                monitor,
                _handler: handler,
            }))
        }
    }

    impl Drop for MacosPressureMonitor {
        fn drop(&mut self) {
            unsafe {
                NSEvent::removeMonitor(&self.monitor);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos_impl::MacosPressureMonitor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_hover_uses_minimum_pressure_and_mouse_uses_full_pressure() {
        let pressure = PressureStateHandle::default();
        assert_eq!(pressure.brush_pressure(), 1.0);

        pressure.set_pen_proximity(true);
        assert_eq!(pressure.brush_pressure(), 0.0);

        pressure.note_pen_pressure(0.4, true, true);
        assert_eq!(pressure.brush_pressure(), 0.4);

        pressure.end_pen_contact(true);
        assert_eq!(pressure.brush_pressure(), 0.0);

        pressure.clear_pen();
        assert_eq!(pressure.brush_pressure(), 1.0);
    }
}
