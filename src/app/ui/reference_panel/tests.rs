use super::*;

#[test]
fn only_selected_unlocked_references_can_be_dragged() {
    assert!(reference_is_draggable(false, true, false));
    assert!(!reference_is_draggable(false, false, false));
    assert!(!reference_is_draggable(true, true, false));
    assert!(!reference_is_draggable(false, true, true));
}
