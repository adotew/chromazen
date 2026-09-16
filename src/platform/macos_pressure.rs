#[cfg(not(target_os = "macos"))]
use std::sync::Arc;
#[cfg(not(target_os = "macos"))]
use winit::window::Window;

use super::PressureStateHandle;

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
    use std::{cell::Cell, ptr::NonNull, sync::Arc, time::Duration};

    use block2::{DynBlock, RcBlock};
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{
        NSEvent, NSEventMask, NSEventSubtype, NSEventType, NSPointingDeviceType, NSView,
    };
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

            // AppKit only allows pointingDeviceType on tablet-proximity events.
            let pen_in_proximity = Cell::new(false);
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

                let changed = match event_type {
                    NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged => {
                        if event.subtype() == NSEventSubtype::TabletPoint {
                            pressure_state.note_pen_pressure_at(
                                event.pressure(),
                                true,
                                true,
                                Some(Duration::from_secs_f64(event.timestamp().max(0.0))),
                            )
                        } else {
                            pressure_state.reset_pen_state()
                        }
                    }
                    NSEventType::LeftMouseUp => {
                        let is_pen_event = event.subtype() == NSEventSubtype::TabletPoint;
                        if is_pen_event {
                            pressure_state.end_pen_contact(true)
                        } else {
                            pressure_state.set_pen_proximity(pen_in_proximity.get())
                        }
                    }
                    NSEventType::MouseCancelled => {
                        pressure_state.set_pen_proximity(pen_in_proximity.get())
                    }
                    NSEventType::TabletPoint => {
                        let pressure = event.pressure();
                        pressure_state.note_pen_pressure_at(
                            pressure,
                            pressure > 0.0,
                            true,
                            Some(Duration::from_secs_f64(event.timestamp().max(0.0))),
                        )
                    }
                    NSEventType::TabletProximity => {
                        let is_pen_device = matches!(
                            event.pointingDeviceType(),
                            NSPointingDeviceType::Pen | NSPointingDeviceType::Eraser
                        );
                        let in_proximity = event.isEnteringProximity() && is_pen_device;
                        pen_in_proximity.set(in_proximity);
                        pressure_state.set_pen_proximity(in_proximity)
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
