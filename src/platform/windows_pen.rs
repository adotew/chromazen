#[cfg(any(target_os = "windows", test))]
use super::PenEvent;

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowsPenAction {
    Down,
    Motion,
    Up,
    Cancel,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WindowsPenSample {
    pub action: WindowsPenAction,
    /// Surface-local logical coordinates.
    pub position: [f32; 2],
    pub pressure: Option<f32>,
    pub contact: bool,
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn events_for_sample(sample: WindowsPenSample) -> Vec<PenEvent> {
    let pressure = effective_pressure(sample.pressure, sample.contact);
    match sample.action {
        WindowsPenAction::Down => vec![PenEvent::Down {
            position: sample.position,
            pressure,
        }],
        WindowsPenAction::Motion => vec![PenEvent::Motion {
            position: sample.position,
            pressure,
            contact: sample.contact,
        }],
        WindowsPenAction::Up => vec![PenEvent::Up],
        WindowsPenAction::Cancel => vec![PenEvent::Up, PenEvent::Leave],
    }
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn normalize_pressure(raw: u32, supported: bool) -> Option<f32> {
    supported.then(|| (raw as f32 / 1024.0).clamp(0.0, 1.0))
}

#[cfg(any(target_os = "windows", test))]
fn effective_pressure(pressure: Option<f32>, contact: bool) -> f32 {
    if contact {
        pressure.unwrap_or(1.0).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(any(target_os = "windows", test))]
fn physical_to_logical(position: [i32; 2], dpi: u32) -> [f32; 2] {
    let scale = 96.0 / dpi.max(96) as f32;
    [position[0] as f32 * scale, position[1] as f32 * scale]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_pressure_range() {
        assert_eq!(normalize_pressure(0, true), Some(0.0));
        assert_eq!(normalize_pressure(512, true), Some(0.5));
        assert_eq!(normalize_pressure(1024, true), Some(1.0));
        assert_eq!(normalize_pressure(2048, true), Some(1.0));
        assert_eq!(normalize_pressure(512, false), None);
    }

    #[test]
    fn pressureless_pen_uses_full_pressure_only_during_contact() {
        let down = WindowsPenSample {
            action: WindowsPenAction::Down,
            position: [12.0, 24.0],
            pressure: None,
            contact: true,
        };
        assert_eq!(
            events_for_sample(down),
            vec![PenEvent::Down {
                position: [12.0, 24.0],
                pressure: 1.0,
            }]
        );

        let hover = WindowsPenSample {
            action: WindowsPenAction::Motion,
            contact: false,
            ..down
        };
        assert_eq!(
            events_for_sample(hover),
            vec![PenEvent::Motion {
                position: [12.0, 24.0],
                pressure: 0.0,
                contact: false,
            }]
        );
    }

    #[test]
    fn release_and_cancellation_end_input() {
        let sample = WindowsPenSample {
            action: WindowsPenAction::Up,
            position: [0.0; 2],
            pressure: Some(0.7),
            contact: false,
        };
        assert_eq!(events_for_sample(sample), vec![PenEvent::Up]);
        assert_eq!(
            events_for_sample(WindowsPenSample {
                action: WindowsPenAction::Cancel,
                ..sample
            }),
            vec![PenEvent::Up, PenEvent::Leave]
        );
    }

    #[test]
    fn physical_coordinates_follow_windows_dpi() {
        assert_eq!(physical_to_logical([300, 150], 96), [300.0, 150.0]);
        assert_eq!(physical_to_logical([300, 150], 144), [200.0, 100.0]);
        assert_eq!(physical_to_logical([300, 150], 0), [300.0, 150.0]);
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use std::sync::Arc;

    use winit::{event_loop::EventLoopBuilder, window::Window};

    use crate::platform::PenEvent;

    #[derive(Clone, Default)]
    pub struct WindowsPenRouter;

    impl WindowsPenRouter {
        pub fn new() -> Self {
            Self
        }

        pub fn install_hook<T: 'static>(&self, _builder: &mut EventLoopBuilder<T>) {}
    }

    pub struct WindowsPenMonitor;

    impl WindowsPenMonitor {
        pub fn install(
            _window: Arc<Window>,
            _router: WindowsPenRouter,
            _sink: impl Fn(PenEvent) + 'static,
        ) -> Result<Option<Self>, String> {
            Ok(None)
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::{
        cell::RefCell,
        panic::{AssertUnwindSafe, catch_unwind},
        ptr,
        rc::Rc,
        sync::Arc,
    };

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::{
        Foundation::HWND,
        Graphics::Gdi::ScreenToClient,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::Pointer::{
                GetPointerPenInfo, GetPointerPenInfoHistory, GetPointerType, POINTER_FLAG_CANCELED,
                POINTER_FLAG_CAPTURECHANGED, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
                POINTER_FLAG_UP, POINTER_PEN_INFO, SkipPointerFrameMessages,
            },
            WindowsAndMessaging::{
                MSG, PEN_MASK_PRESSURE, PT_PEN, WM_POINTERCAPTURECHANGED, WM_POINTERDOWN,
                WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE,
            },
        },
    };
    use winit::{
        event_loop::EventLoopBuilder, platform::windows::EventLoopBuilderExtWindows, window::Window,
    };

    use super::{
        WindowsPenAction, WindowsPenSample, events_for_sample, normalize_pressure,
        physical_to_logical,
    };
    use crate::platform::PenEvent;

    type PenSink = Box<dyn Fn(PenEvent)>;

    #[derive(Clone, Default)]
    pub struct WindowsPenRouter {
        state: Rc<RefCell<RouterState>>,
    }

    #[derive(Default)]
    struct RouterState {
        target_hwnd: Option<usize>,
        active_pointer: Option<u32>,
        active_contact: bool,
        sink: Option<PenSink>,
    }

    pub struct WindowsPenMonitor {
        router: WindowsPenRouter,
        hwnd: usize,
    }

    impl WindowsPenRouter {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn install_hook<T: 'static>(&self, builder: &mut EventLoopBuilder<T>) {
            let state = Rc::clone(&self.state);
            builder.with_msg_hook(move |message| {
                catch_unwind(AssertUnwindSafe(|| {
                    // Safety: winit passes a valid MSG pointer for the duration of this callback.
                    let message = unsafe { &*(message.cast::<MSG>()) };
                    state.borrow_mut().handle_message(message)
                }))
                .unwrap_or_else(|_| {
                    log::error!("Windows pen message handler panicked");
                    false
                })
            });
        }
    }

    impl WindowsPenMonitor {
        pub fn install(
            window: Arc<Window>,
            router: WindowsPenRouter,
            sink: impl Fn(PenEvent) + 'static,
        ) -> Result<Option<Self>, String> {
            let window_handle = window
                .window_handle()
                .map_err(|error| format!("failed to get window handle: {error}"))?;
            let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
                return Err("expected a Win32 window handle on Windows".into());
            };
            let hwnd = handle.hwnd.get() as usize;
            let mut state = router.state.borrow_mut();
            state.target_hwnd = Some(hwnd);
            state.active_pointer = None;
            state.active_contact = false;
            state.sink = Some(Box::new(sink));
            drop(state);
            Ok(Some(Self { router, hwnd }))
        }
    }

    impl Drop for WindowsPenMonitor {
        fn drop(&mut self) {
            let mut state = self.router.state.borrow_mut();
            if state.target_hwnd == Some(self.hwnd) {
                state.target_hwnd = None;
                state.active_pointer = None;
                state.active_contact = false;
                state.sink = None;
            }
        }
    }

    impl RouterState {
        fn handle_message(&mut self, message: &MSG) -> bool {
            if self.target_hwnd != Some(message.hwnd as usize) {
                return false;
            }

            let pointer_id = loword(message.wParam);
            match message.message {
                WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERENTER => {
                    if !is_pen(pointer_id) {
                        return false;
                    }
                    self.active_pointer = Some(pointer_id);
                    let infos = pen_history(pointer_id);
                    for info in infos.iter().rev() {
                        self.emit_info(message.hwnd, info);
                    }
                    // Prevent queued frame messages from replaying samples we emitted from history.
                    unsafe {
                        SkipPointerFrameMessages(pointer_id);
                    }
                    true
                }
                WM_POINTERLEAVE if self.active_pointer == Some(pointer_id) => {
                    self.end_pointer();
                    true
                }
                WM_POINTERCAPTURECHANGED if self.active_pointer == Some(pointer_id) => {
                    self.end_pointer();
                    true
                }
                _ => false,
            }
        }

        fn emit_info(&mut self, hwnd: HWND, info: &POINTER_PEN_INFO) {
            let flags = info.pointerInfo.pointerFlags;
            let action = if flags & (POINTER_FLAG_CANCELED | POINTER_FLAG_CAPTURECHANGED) != 0 {
                WindowsPenAction::Cancel
            } else if flags & POINTER_FLAG_DOWN != 0 {
                WindowsPenAction::Down
            } else if flags & POINTER_FLAG_UP != 0 {
                WindowsPenAction::Up
            } else {
                WindowsPenAction::Motion
            };
            let mut point = info.pointerInfo.ptPixelLocation;
            if unsafe { ScreenToClient(hwnd, &mut point) } == 0 {
                return;
            }
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            let contact = flags & POINTER_FLAG_INCONTACT != 0;
            self.active_contact = contact;
            self.emit_sample(WindowsPenSample {
                action,
                position: physical_to_logical([point.x, point.y], dpi),
                pressure: normalize_pressure(info.pressure, info.penMask & PEN_MASK_PRESSURE != 0),
                contact,
            });
            if action == WindowsPenAction::Cancel {
                self.active_pointer = None;
                self.active_contact = false;
            }
        }

        fn end_pointer(&mut self) {
            if self.active_contact {
                self.emit_event(PenEvent::Up);
            }
            self.emit_event(PenEvent::Leave);
            self.active_pointer = None;
            self.active_contact = false;
        }

        fn emit_sample(&self, sample: WindowsPenSample) {
            for event in events_for_sample(sample) {
                self.emit_event(event);
            }
        }

        fn emit_event(&self, event: PenEvent) {
            if let Some(sink) = self.sink.as_ref() {
                sink(event);
            }
        }
    }

    fn loword(value: usize) -> u32 {
        (value & 0xffff) as u32
    }

    fn is_pen(pointer_id: u32) -> bool {
        let mut pointer_type = 0;
        unsafe { GetPointerType(pointer_id, &mut pointer_type) != 0 && pointer_type == PT_PEN }
    }

    fn pen_history(pointer_id: u32) -> Vec<POINTER_PEN_INFO> {
        const MAX_HISTORY: u32 = 4096;

        let mut count = 0;
        if unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, ptr::null_mut()) } != 0
            && count > 0
            && count <= MAX_HISTORY
        {
            let mut infos = vec![POINTER_PEN_INFO::default(); count as usize];
            if unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, infos.as_mut_ptr()) } != 0
            {
                infos.truncate(count as usize);
                return infos;
            }
        }

        let mut info = POINTER_PEN_INFO::default();
        if unsafe { GetPointerPenInfo(pointer_id, &mut info) } != 0 {
            vec![info]
        } else {
            Vec::new()
        }
    }
}

pub(crate) use imp::{WindowsPenMonitor, WindowsPenRouter};
