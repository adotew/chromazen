mod macos_pressure;
mod pressure;
mod wayland_tablet;

pub(crate) use macos_pressure::MacosPressureMonitor;
pub(crate) use pressure::PressureStateHandle;
pub(crate) use wayland_tablet::WaylandTabletMonitor;

/// Platform-neutral pen/tablet stroke event produced by platform monitors
/// (e.g. the Wayland tablet monitor) and consumed by the app event loop.
///
/// Positions are surface-local logical coordinates; call sites multiply them by
/// the window scale factor to match winit's physical-pixel cursor events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PenEvent {
    Down { position: [f32; 2], pressure: f32 },
    Move { position: [f32; 2], pressure: f32 },
    Up,
    Leave,
}
