#[cfg(not(target_os = "linux"))]
mod imp {
    use std::sync::Arc;

    use winit::window::Window;

    use crate::platform::PenEvent;

    pub struct WaylandTabletMonitor;

    impl WaylandTabletMonitor {
        pub fn set_cursor_visible(&self, _visible: bool) {}

        pub fn install(
            _window: Arc<Window>,
            _sink: impl Fn(PenEvent) + Send + 'static,
        ) -> Result<Option<Self>, String> {
            Ok(None)
        }
    }
}

/// Graphics tablet support via the Wayland `tablet-v2` protocol.
///
/// winit does not deliver tablet input on Linux, so this monitor borrows the
/// window's existing Wayland connection, binds `zwp_tablet_manager_v2`, and
/// translates tool events (tip, motion, pressure) into platform-neutral
/// [`PenEvent`]s that the app feeds through its regular mouse input pipeline.
#[cfg(target_os = "linux")]
mod imp {
    use std::os::fd::OwnedFd;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle,
        backend::Backend,
        protocol::{
            wl_registry,
            wl_seat::{self, WlSeat},
        },
    };
    use wayland_protocols::wp::cursor_shape::v1::client::{
        wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
        wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
    };
    use wayland_protocols::wp::tablet::zv2::client::{
        zwp_tablet_manager_v2::{self, ZwpTabletManagerV2},
        zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
        zwp_tablet_pad_ring_v2::{self, ZwpTabletPadRingV2},
        zwp_tablet_pad_strip_v2::{self, ZwpTabletPadStripV2},
        zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
        zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
        zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
        zwp_tablet_v2::{self, ZwpTabletV2},
    };
    use winit::window::Window;

    use crate::platform::{MillisecondClock, PenEvent};

    pub struct WaylandTabletMonitor {
        shutdown: Arc<AtomicBool>,
        cursor_visible: Arc<AtomicBool>,
        wake: Arc<OwnedFd>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl WaylandTabletMonitor {
        pub fn set_cursor_visible(&self, visible: bool) {
            if self.cursor_visible.swap(visible, Ordering::AcqRel) != visible
                && let Err(error) = rustix::io::write(&*self.wake, &1u64.to_ne_bytes())
            {
                log::debug!("failed to wake tablet thread: {error}");
            }
        }

        pub fn install(
            window: Arc<Window>,
            sink: impl Fn(PenEvent) + Send + 'static,
        ) -> Result<Option<Self>, String> {
            let display_handle = window
                .display_handle()
                .map_err(|error| format!("failed to get display handle: {error}"))?;
            let RawDisplayHandle::Wayland(display_handle) = display_handle.as_raw() else {
                // X11 session: the pen driver feeds the core pointer, so the
                // tablet already behaves like a mouse. Nothing to do here.
                return Ok(None);
            };
            let window_handle = window
                .window_handle()
                .map_err(|error| format!("failed to get window handle: {error}"))?;
            let RawWindowHandle::Wayland(surface_handle) = window_handle.as_raw() else {
                return Ok(None);
            };
            let window_surface = surface_handle.surface.as_ptr() as usize;

            // Safety: the wl_display is owned by winit's event loop, which
            // outlives this monitor (the dispatch thread exits when the display
            // disconnects at shutdown).
            let backend =
                unsafe { Backend::from_foreign_display(display_handle.display.as_ptr().cast()) };
            let connection = Connection::from_backend(backend);
            let display = connection.display();
            let mut queue = connection.new_event_queue::<TabletState>();
            let qh = queue.handle();
            let registry = display.get_registry(&qh, ());

            let mut state = TabletState::new(window_surface, registry, sink);
            queue
                .roundtrip(&mut state)
                .map_err(|error| format!("failed to query Wayland globals: {error}"))?;
            if state.manager.is_none() {
                log::debug!(
                    "compositor does not support zwp_tablet_manager_v2; tablet input disabled"
                );
                return Ok(None);
            }

            let cursor_visible = Arc::clone(&state.cursor_visible);
            let wake = Arc::new(
                eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                    .map_err(|error| format!("failed to create tablet wake eventfd: {error}"))?,
            );
            let thread_wake = Arc::clone(&wake);
            let shutdown = Arc::new(AtomicBool::new(false));
            let thread_shutdown = Arc::clone(&shutdown);
            let thread = std::thread::Builder::new()
                .name("chromazen-tablet".to_owned())
                .spawn(move || {
                    dispatch_tablet_events(connection, queue, state, &thread_wake, thread_shutdown)
                })
                .map_err(|error| format!("failed to spawn tablet event thread: {error}"))?;

            Ok(Some(Self {
                shutdown,
                cursor_visible,
                wake,
                thread: Some(thread),
            }))
        }
    }

    impl Drop for WaylandTabletMonitor {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take()
                && thread.join().is_err()
            {
                log::warn!("Wayland tablet thread panicked during shutdown");
            }
        }
    }

    fn dispatch_tablet_events(
        connection: Connection,
        mut queue: wayland_client::EventQueue<TabletState>,
        mut state: TabletState,
        wake: &OwnedFd,
        shutdown: Arc<AtomicBool>,
    ) {
        const POLL_TIMEOUT: Timespec = Timespec {
            tv_sec: 0,
            tv_nsec: 50_000_000,
        };

        while !shutdown.load(Ordering::Acquire) {
            match queue.dispatch_pending(&mut state) {
                Ok(_) => {}
                Err(error) => {
                    log::debug!("tablet event dispatch ended: {error}");
                    break;
                }
            }
            state.update_tool_cursors(&queue.handle());
            if let Err(error) = queue.flush() {
                log::debug!("tablet event flush ended: {error}");
                break;
            }

            let Some(read_guard) = queue.prepare_read() else {
                continue;
            };
            let backend = connection.backend();
            let mut poll_fds = [
                PollFd::from_borrowed_fd(backend.poll_fd(), PollFlags::IN),
                PollFd::new(wake, PollFlags::IN),
            ];
            match poll(&mut poll_fds, Some(&POLL_TIMEOUT)) {
                Ok(_) => {
                    // Reset the eventfd counter; the next loop pass applies the new visibility.
                    if poll_fds[1].revents().contains(PollFlags::IN) {
                        let _ = rustix::io::read(wake, &mut [0; 8]);
                    }
                    if poll_fds[0].revents().contains(PollFlags::IN)
                        && let Err(error) = read_guard.read()
                    {
                        log::debug!("tablet event read ended: {error}");
                        break;
                    }
                }
                Err(error) => {
                    drop(read_guard);
                    log::debug!("tablet event poll ended: {error}");
                    break;
                }
            }
        }
    }

    struct TabletState {
        /// Raw `wl_surface` pointer of the app window, used to ignore tools that
        /// are in proximity over other surfaces. Stored as `usize` for `Send`.
        window_surface: usize,
        sink: Box<dyn Fn(PenEvent) + Send>,
        _registry: wl_registry::WlRegistry,
        manager: Option<ZwpTabletManagerV2>,
        pending_seats: Vec<WlSeat>,
        seats: Vec<WlSeat>,
        tablet_seats: Vec<ZwpTabletSeatV2>,
        tablets: Vec<ZwpTabletV2>,
        tools: Vec<ToolCursor>,
        cursor_shape_manager: Option<WpCursorShapeManagerV1>,
        cursor_visible: Arc<AtomicBool>,
        clock: MillisecondClock,
        frame: TabletFrame,
    }

    struct ToolCursor {
        tool: ZwpTabletToolV2,
        shape_device: Option<WpCursorShapeDeviceV1>,
        visibility: ToolCursorVisibility,
    }

    #[derive(Default)]
    struct ToolCursorVisibility {
        enter_serial: Option<u32>,
        applied: Option<bool>,
    }

    impl ToolCursorVisibility {
        fn enter(&mut self, serial: Option<u32>) {
            self.enter_serial = serial;
            self.applied = None;
        }

        fn update_serial(&self, visible: bool) -> Option<u32> {
            self.enter_serial.filter(|_| self.applied != Some(visible))
        }
    }

    #[derive(Debug, Default)]
    struct TabletFrame {
        proximity: bool,
        tip_down: bool,
        position: [f32; 2],
        pressure: f32,
        pressure_supported: bool,
        action: FrameAction,
        updated: bool,
        time: std::time::Duration,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    enum FrameAction {
        #[default]
        None,
        Down,
        Up,
        Leave,
    }

    impl TabletState {
        fn new(
            window_surface: usize,
            registry: wl_registry::WlRegistry,
            sink: impl Fn(PenEvent) + Send + 'static,
        ) -> Self {
            Self {
                window_surface,
                sink: Box::new(sink),
                _registry: registry,
                manager: None,
                pending_seats: Vec::new(),
                seats: Vec::new(),
                tablet_seats: Vec::new(),
                tablets: Vec::new(),
                tools: Vec::new(),
                cursor_shape_manager: None,
                cursor_visible: Arc::new(AtomicBool::new(true)),
                clock: MillisecondClock::default(),
                frame: TabletFrame::default(),
            }
        }

        fn update_tool_cursors(&mut self, qh: &QueueHandle<Self>) {
            // Without cursor-shape support, hiding would leave no way to restore the cursor.
            let Some(manager) = &self.cursor_shape_manager else {
                return;
            };
            let visible = self.cursor_visible.load(Ordering::Acquire);
            for cursor in &mut self.tools {
                let Some(serial) = cursor.visibility.update_serial(visible) else {
                    continue;
                };
                if visible {
                    cursor
                        .shape_device
                        .get_or_insert_with(|| manager.get_tablet_tool_v2(&cursor.tool, qh, ()))
                        .set_shape(serial, Shape::Default);
                } else {
                    cursor.tool.set_cursor(serial, None, 0, 0);
                }
                cursor.visibility.applied = Some(visible);
            }
        }

        fn enter_tool_cursor(&mut self, tool: &ZwpTabletToolV2, serial: Option<u32>) {
            if let Some(cursor) = self.tools.iter_mut().find(|cursor| cursor.tool == *tool) {
                cursor.visibility.enter(serial);
            }
        }

        fn bind_pending_seats(&mut self, qh: &QueueHandle<Self>) {
            let Some(manager) = &self.manager else {
                return;
            };
            for seat in self.pending_seats.drain(..) {
                self.tablet_seats
                    .push(manager.get_tablet_seat(&seat, qh, ()));
                self.seats.push(seat);
            }
        }

        fn emit_pen_event(&self, event: PenEvent) {
            (self.sink)(event);
        }

        fn emit_pending_frame_event(&mut self, milliseconds: u32) {
            self.frame.time = self.clock.observe(milliseconds);
            if let Some(event) = self.frame.take_pen_event() {
                self.emit_pen_event(event);
            }
        }
    }

    impl TabletFrame {
        fn effective_pressure(&self) -> f32 {
            if !self.tip_down {
                0.0
            } else if self.pressure_supported {
                self.pressure
            } else {
                1.0
            }
        }

        fn take_pen_event(&mut self) -> Option<PenEvent> {
            let event = match self.action {
                FrameAction::Leave => Some(PenEvent::Leave),
                FrameAction::Up => Some(PenEvent::Up),
                FrameAction::Down => Some(PenEvent::Down {
                    position: self.position,
                    pressure: self.effective_pressure(),
                    time: self.time,
                }),
                FrameAction::None if self.proximity && self.updated => Some(PenEvent::Motion {
                    position: self.position,
                    pressure: self.effective_pressure(),
                    contact: self.tip_down,
                    time: self.time,
                }),
                FrameAction::None => None,
            };
            if matches!(self.action, FrameAction::Up | FrameAction::Leave) {
                self.pressure = 0.0;
            }
            self.action = FrameAction::None;
            self.updated = false;
            event
        }
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for TabletState {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            qh: &QueueHandle<Self>,
        ) {
            let wl_registry::Event::Global {
                name,
                interface,
                version,
            } = event
            else {
                return;
            };
            match interface.as_str() {
                "zwp_tablet_manager_v2" => {
                    let manager =
                        registry.bind::<ZwpTabletManagerV2, _, _>(name, version.min(1), qh, ());
                    state.manager = Some(manager);
                    state.bind_pending_seats(qh);
                }
                "wp_cursor_shape_manager_v1" => {
                    state.cursor_shape_manager = Some(registry.bind(name, 1, qh, ()));
                }
                "wl_seat" => {
                    let seat = registry.bind::<WlSeat, _, _>(name, version.min(1), qh, ());
                    state.pending_seats.push(seat);
                    state.bind_pending_seats(qh);
                }
                _ => {}
            }
        }
    }

    impl Dispatch<ZwpTabletManagerV2, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &ZwpTabletManagerV2,
            _: zwp_tablet_manager_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    wayland_client::delegate_noop!(TabletState: ignore WpCursorShapeManagerV1);
    wayland_client::delegate_noop!(TabletState: ignore WpCursorShapeDeviceV1);

    impl Dispatch<WlSeat, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &WlSeat,
            _: wl_seat::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwpTabletSeatV2, ()> for TabletState {
        fn event(
            state: &mut Self,
            _: &ZwpTabletSeatV2,
            event: zwp_tablet_seat_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwp_tablet_seat_v2::Event::TabletAdded { id } => state.tablets.push(id),
                zwp_tablet_seat_v2::Event::ToolAdded { id } => state.tools.push(ToolCursor {
                    tool: id,
                    shape_device: None,
                    visibility: ToolCursorVisibility::default(),
                }),
                _ => {}
            }
        }

        // These events deliver newly created objects; the macro tells
        // wayland-client how to initialize their proxies.
        wayland_client::event_created_child!(TabletState, ZwpTabletSeatV2, [
            zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
            zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
            zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
        ]);
    }

    impl Dispatch<ZwpTabletV2, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &ZwpTabletV2,
            _: zwp_tablet_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    // Tablet pads (buttons/rings/strips on the tablet itself) are not used, but
    // their events create child objects, so the whole pad object tree needs
    // no-op handlers to keep wayland-client from panicking.
    impl Dispatch<ZwpTabletPadV2, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &ZwpTabletPadV2,
            _: zwp_tablet_pad_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }

        wayland_client::event_created_child!(TabletState, ZwpTabletPadV2, [
            zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
        ]);
    }

    impl Dispatch<ZwpTabletPadGroupV2, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &ZwpTabletPadGroupV2,
            _: zwp_tablet_pad_group_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }

        wayland_client::event_created_child!(TabletState, ZwpTabletPadGroupV2, [
            zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
            zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
        ]);
    }

    impl Dispatch<ZwpTabletPadRingV2, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &ZwpTabletPadRingV2,
            _: zwp_tablet_pad_ring_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwpTabletPadStripV2, ()> for TabletState {
        fn event(
            _: &mut Self,
            _: &ZwpTabletPadStripV2,
            _: zwp_tablet_pad_strip_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwpTabletToolV2, ()> for TabletState {
        fn event(
            state: &mut Self,
            tool: &ZwpTabletToolV2,
            event: zwp_tablet_tool_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwp_tablet_tool_v2::Event::ProximityIn {
                    surface, serial, ..
                } => {
                    state.frame.proximity = surface.id().as_ptr() as usize == state.window_surface;
                    state.enter_tool_cursor(tool, state.frame.proximity.then_some(serial));
                    state.frame.tip_down = false;
                    state.frame.pressure = 0.0;
                }
                zwp_tablet_tool_v2::Event::ProximityOut if state.frame.proximity => {
                    state.frame.proximity = false;
                    state.enter_tool_cursor(tool, None);
                    state.frame.tip_down = false;
                    state.frame.action = FrameAction::Leave;
                }
                zwp_tablet_tool_v2::Event::Motion { x, y } if state.frame.proximity => {
                    state.frame.position = [x as f32, y as f32];
                    state.frame.updated = true;
                }
                zwp_tablet_tool_v2::Event::Pressure { pressure } if state.frame.proximity => {
                    state.frame.pressure = (pressure as f32 / 65535.0).clamp(0.0, 1.0);
                    state.frame.pressure_supported = true;
                    state.frame.updated = true;
                }
                zwp_tablet_tool_v2::Event::Down { .. } if state.frame.proximity => {
                    state.frame.tip_down = true;
                    state.frame.action = FrameAction::Down;
                }
                zwp_tablet_tool_v2::Event::Up if state.frame.tip_down => {
                    state.frame.tip_down = false;
                    state.frame.action = FrameAction::Up;
                }
                zwp_tablet_tool_v2::Event::Frame { time } => {
                    state.emit_pending_frame_event(time);
                }
                _ => {}
            }
        }
    }

    #[cfg(test)]
    mod tests;
}

pub(crate) use imp::WaylandTabletMonitor;
