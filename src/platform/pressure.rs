use std::sync::{Arc, Mutex};

#[cfg(not(target_os = "macos"))]
use winit::window::Window;

#[derive(Clone, Debug, Default)]
pub struct PressureStateHandle(Arc<Mutex<PressureState>>);

#[derive(Debug)]
struct PressureState {
    pressure: f32,
    pen_active: bool,
}

impl Default for PressureState {
    fn default() -> Self {
        Self {
            pressure: 1.0,
            pen_active: false,
        }
    }
}

impl PressureStateHandle {
    pub fn brush_pressure(&self) -> f32 {
        let state = self.0.lock().expect("pressure state poisoned");
        if state.pen_active {
            state.pressure
        } else {
            1.0
        }
    }

    pub fn is_pen_active(&self) -> bool {
        self.0.lock().expect("pressure state poisoned").pen_active
    }

    fn note_pen_pressure(&self, pressure: f32, active: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let pressure = pressure.clamp(0.0, 1.0);
        let changed =
            state.pen_active != active || (state.pressure - pressure).abs() > f32::EPSILON;
        state.pen_active = active;
        state.pressure = pressure;
        changed
    }

    fn clear_pen(&self) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let changed = state.pen_active || (state.pressure - 1.0).abs() > f32::EPSILON;
        state.pen_active = false;
        state.pressure = 1.0;
        changed
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
                let Some(event_window) = event.window(mtm) else {
                    return event_ptr.as_ptr();
                };
                if !std::ptr::eq(&*event_window, &*ns_window) {
                    return event_ptr.as_ptr();
                }

                let event_type = event.r#type();
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
                            pressure_state.note_pen_pressure(pressure, true)
                        } else {
                            pressure_state.clear_pen()
                        }
                    }
                    NSEventType::LeftMouseUp | NSEventType::MouseCancelled => {
                        pressure_state.clear_pen()
                    }
                    NSEventType::TabletPoint | NSEventType::Pressure => {
                        if should_use_pressure {
                            pressure_state.note_pen_pressure(pressure, true)
                        } else {
                            false
                        }
                    }
                    NSEventType::TabletProximity => {
                        if event.isEnteringProximity() {
                            false
                        } else {
                            pressure_state.clear_pen()
                        }
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
