use std::time::Duration;

mod macos_pressure;
mod pressure;
mod wayland_tablet;
mod windows_pen;

pub(crate) use macos_pressure::MacosPressureMonitor;
pub(crate) use pressure::PressureStateHandle;
pub(crate) use wayland_tablet::WaylandTabletMonitor;
pub(crate) use windows_pen::{WindowsPenMonitor, WindowsPenRouter};

#[cfg(any(target_os = "linux", target_os = "windows", test))]
#[derive(Debug, Default)]
struct MillisecondClock {
    last: Option<u32>,
    elapsed: Duration,
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
impl MillisecondClock {
    fn observe(&mut self, milliseconds: u32) -> Duration {
        if let Some(last) = self.last {
            self.elapsed += Duration::from_millis(u64::from(milliseconds.wrapping_sub(last)));
        }
        self.last = Some(milliseconds);
        self.elapsed
    }
}

/// Platform-neutral tablet input in surface-local logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
pub(crate) enum PenEvent {
    Down {
        position: [f32; 2],
        pressure: f32,
        time: Duration,
    },
    Motion {
        position: [f32; 2],
        pressure: f32,
        contact: bool,
        time: Duration,
    },
    Up,
    Leave,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millisecond_clock_unwraps_rollover() {
        let mut clock = MillisecondClock::default();
        assert_eq!(clock.observe(u32::MAX - 2), Duration::ZERO);
        assert_eq!(clock.observe(1), Duration::from_millis(4));
    }
}
