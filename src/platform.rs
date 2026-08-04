mod macos_pressure;
mod pressure;
mod wayland_tablet;
mod windows_pen;

pub(crate) use macos_pressure::MacosPressureMonitor;
pub(crate) use pressure::PressureStateHandle;
pub(crate) use wayland_tablet::WaylandTabletMonitor;

/// Platform-neutral tablet input in surface-local logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum PenEvent {
    Down {
        position: [f32; 2],
        pressure: f32,
    },
    Motion {
        position: [f32; 2],
        pressure: f32,
        contact: bool,
    },
    Up,
    Leave,
}
