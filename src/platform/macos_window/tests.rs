use super::*;

#[test]
fn controls_center_is_inset_from_the_top_in_both_coordinate_systems() {
    assert_eq!(controls_center_y(900.0, true, WINDOW_CONTROLS_MARGIN), 33.0);
    assert_eq!(
        controls_center_y(900.0, false, WINDOW_CONTROLS_MARGIN),
        867.0
    );
    assert_eq!(controls_center_y(900.0, true, 0.0), 21.0);
    assert_eq!(controls_center_y(900.0, false, 0.0), 879.0);
}
