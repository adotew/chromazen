use super::*;

#[test]
fn row_alignment_rounds_up_to_wgpu_requirement() {
    assert_eq!(aligned_bytes_per_row(16_000), 16_128);
    assert_eq!(aligned_bytes_per_row(256), 256);
}

#[test]
fn row_padding_is_removed() {
    let source = [1, 2, 3, 4, 9, 9, 5, 6, 7, 8, 9, 9];
    assert_eq!(unpack_rows(&source, 4, 6, 2), vec![1, 2, 3, 4, 5, 6, 7, 8]);
}
