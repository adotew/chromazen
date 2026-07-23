use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

#[derive(Clone, Debug, Default)]
pub struct PressureStateHandle(Arc<PressureState>);

#[derive(Debug)]
struct PressureState {
    pressure_bits: AtomicU32,
    pen_active: AtomicBool,
}

impl Default for PressureState {
    fn default() -> Self {
        Self {
            pressure_bits: AtomicU32::new(1.0_f32.to_bits()),
            pen_active: AtomicBool::new(false),
        }
    }
}

impl PressureStateHandle {
    pub fn brush_pressure(&self) -> f32 {
        if self.0.pen_active.load(Ordering::Relaxed) {
            f32::from_bits(self.0.pressure_bits.load(Ordering::Relaxed))
        } else {
            1.0
        }
    }

    fn note_pen_pressure(&self, pressure: f32, active: bool) -> bool {
        let pressure = pressure.clamp(0.0, 1.0);
        let previous_active = self.0.pen_active.swap(active, Ordering::Relaxed);
        let previous_pressure = f32::from_bits(
            self.0
                .pressure_bits
                .swap(pressure.to_bits(), Ordering::Relaxed),
        );
        previous_active != active || (previous_pressure - pressure).abs() > f32::EPSILON
    }

    pub fn clear_pen(&self) -> bool {
        let previous_active = self.0.pen_active.swap(false, Ordering::Relaxed);
        let previous_pressure = f32::from_bits(
            self.0
                .pressure_bits
                .swap(1.0_f32.to_bits(), Ordering::Relaxed),
        );
        previous_active || (previous_pressure - 1.0).abs() > f32::EPSILON
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MacosPressureMonitor;

#[cfg(not(target_os = "macos"))]
impl MacosPressureMonitor {
    pub fn install<W, F>(
        _window: Arc<W>,
        _pressure_state: PressureStateHandle,
        _request_redraw: F,
    ) -> Result<Option<Self>, String>
    where
        W: Send + Sync + 'static,
        F: Fn() + Send + Sync + 'static,
    {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::{ptr::NonNull, sync::Arc};

    use super::PressureStateHandle;
    use block2::{DynBlock, RcBlock};
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventSubtype, NSEventType, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub struct MacosPressureMonitor {
        monitor: Retained<AnyObject>,
        _handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>,
    }

    impl MacosPressureMonitor {
        pub fn install<W, F>(
            window: Arc<W>,
            pressure_state: PressureStateHandle,
            request_redraw: F,
        ) -> Result<Option<Self>, String>
        where
            W: HasWindowHandle + Send + Sync + 'static,
            F: Fn() + Send + Sync + 'static,
        {
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
                match event.window(mtm) {
                    Some(event_window) if !std::ptr::eq(&*event_window, &*ns_window) => {
                        return event_ptr.as_ptr();
                    }
                    // Tablet-proximity events are not necessarily associated with
                    // a window. Other windowless events cannot belong to this
                    // monitor's paint window.
                    None if event_type != NSEventType::TabletProximity => {
                        return event_ptr.as_ptr();
                    }
                    _ => {}
                }

                // AppKit event accessors are only valid for particular event
                // types. Querying tablet-only properties on an ordinary mouse
                // event raises an Objective-C exception, which cannot unwind
                // through Tao's `sendEvent:` callback and aborts the process.
                let changed = match event_type {
                    NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged => {
                        if event.subtype() == NSEventSubtype::TabletPoint {
                            pressure_state.note_pen_pressure(event.pressure(), true)
                        } else {
                            pressure_state.clear_pen()
                        }
                    }
                    // Tao emits a final CursorMoved before MouseInput::Released.
                    // Keep the last pressure until the input controller finishes the stroke.
                    NSEventType::LeftMouseUp => false,
                    NSEventType::MouseCancelled => pressure_state.clear_pen(),
                    NSEventType::TabletPoint => {
                        pressure_state.note_pen_pressure(event.pressure(), true)
                    }
                    NSEventType::Pressure => {
                        pressure_state.note_pen_pressure(event.pressure(), true)
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
                    request_redraw();
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
    fn pen_pressure_is_retained_until_input_clears_it() {
        let state = PressureStateHandle::default();
        state.note_pen_pressure(0.25, true);

        assert_eq!(state.brush_pressure(), 0.25);
        assert!(state.clear_pen());
        assert_eq!(state.brush_pressure(), 1.0);
    }
}
