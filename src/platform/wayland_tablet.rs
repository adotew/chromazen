#[cfg(not(target_os = "linux"))]
mod imp {
    use std::sync::Arc;

    use winit::window::Window;

    use crate::platform::PenEvent;

    pub struct WaylandTabletMonitor;

    impl WaylandTabletMonitor {
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
    use std::sync::Arc;

    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle,
        backend::Backend,
        protocol::{
            wl_registry,
            wl_seat::{self, WlSeat},
        },
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

    use crate::platform::PenEvent;

    pub struct WaylandTabletMonitor {
        _thread: std::thread::JoinHandle<()>,
    }

    impl WaylandTabletMonitor {
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

            let thread = std::thread::Builder::new()
                .name("chromazen-tablet".to_owned())
                .spawn(move || {
                    loop {
                        if let Err(error) = queue.blocking_dispatch(&mut state) {
                            log::debug!("tablet event dispatch ended: {error}");
                            break;
                        }
                    }
                })
                .map_err(|error| format!("failed to spawn tablet event thread: {error}"))?;

            Ok(Some(Self { _thread: thread }))
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
        tools: Vec<ZwpTabletToolV2>,
        proximity: bool,
        tip_down: bool,
        position: [f32; 2],
        /// Last reported tool pressure; stays 1.0 if the tool never reports
        /// pressure so such pens behave like a plain mouse.
        pressure: f32,
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
                proximity: false,
                tip_down: false,
                position: [0.0; 2],
                pressure: 1.0,
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

        fn emit(&self, event: PenEvent) {
            (self.sink)(event);
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
                zwp_tablet_seat_v2::Event::ToolAdded { id } => state.tools.push(id),
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
            _: &ZwpTabletToolV2,
            event: zwp_tablet_tool_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwp_tablet_tool_v2::Event::ProximityIn { surface, .. } => {
                    state.proximity = surface.id().as_ptr() as usize == state.window_surface;
                    state.tip_down = false;
                }
                zwp_tablet_tool_v2::Event::ProximityOut if state.proximity => {
                    state.proximity = false;
                    state.tip_down = false;
                    state.emit(PenEvent::Leave);
                }
                zwp_tablet_tool_v2::Event::Motion { x, y } => {
                    state.position = [x as f32, y as f32];
                    if state.proximity {
                        state.emit(PenEvent::Move {
                            position: state.position,
                            pressure: state.pressure,
                        });
                    }
                }
                zwp_tablet_tool_v2::Event::Pressure { pressure } => {
                    state.pressure = (pressure as f32 / 65535.0).clamp(0.0, 1.0);
                }
                zwp_tablet_tool_v2::Event::Down { .. } => {
                    if state.proximity {
                        state.tip_down = true;
                        state.emit(PenEvent::Down {
                            position: state.position,
                            pressure: state.pressure,
                        });
                    }
                }
                zwp_tablet_tool_v2::Event::Up if state.tip_down => {
                    state.tip_down = false;
                    state.emit(PenEvent::Up);
                }
                _ => {}
            }
        }
    }
}

pub(crate) use imp::WaylandTabletMonitor;
