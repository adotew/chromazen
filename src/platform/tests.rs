use super::*;

#[test]
fn millisecond_clock_unwraps_rollover() {
    let mut clock = MillisecondClock::default();
    assert_eq!(clock.observe(u32::MAX - 2), Duration::ZERO);
    assert_eq!(clock.observe(1), Duration::from_millis(4));
}
