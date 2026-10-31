use super::*;

#[test]
fn parses_versions_seven_and_nine() {
    for version in [7, 9] {
        let abr = modern_abr_version(version, 1, modern_block(1, 1, 8, 0, &[255]));
        let parsed = parse_abr(&abr).expect("supported modern ABR");

        assert_eq!(parsed.brushes.len(), 1);
        assert_eq!(parsed.brushes[0].mask, vec![255]);
    }
}

#[test]
fn parses_modern_raw_sample() {
    let abr = modern_abr(1, modern_block(1, 2, 8, 0, &[10, 20]));

    let parsed = parse_abr(&abr).expect("modern ABR");

    assert_eq!(parsed.brushes.len(), 1);
    assert_eq!(parsed.brushes[0].mask, vec![10, 20]);
    assert_eq!((parsed.brushes[0].width, parsed.brushes[0].height), (2, 1));
    assert_eq!(parsed.brushes[0].spacing_percent, None);
}

#[test]
fn parses_subversion_two_16_bit_sample() {
    let abr = modern_abr(2, modern_block_with_prefix(301, 1, 1, 16, 0, &[128, 0]));

    let parsed = parse_abr(&abr).expect("modern 16-bit ABR");

    assert_eq!(parsed.brushes[0].mask, vec![128]);
}

#[test]
fn ignores_descriptor_sections() {
    let mut abr = modern_abr(1, modern_block(1, 1, 8, 0, &[255]));
    let descriptor = [
        0, 0, 0, 16, // descriptor version
        0, 0, 0, 0, // empty Unicode class name
        0xff, 0xff, 0xff, 0xff, // invalid signed class ID length
    ];
    abr.extend_from_slice(b"8BIMdesc");
    abr.extend_from_slice(&(descriptor.len() as u32).to_be_bytes());
    abr.extend_from_slice(&descriptor);

    let parsed = parse_abr(&abr).expect("sampled brush with ignored descriptor");

    assert_eq!(parsed.brushes.len(), 1);
    assert_eq!(parsed.brushes[0].name, None);
    assert_eq!(parsed.brushes[0].spacing_percent, None);
}

#[test]
fn parses_packbits_rows() {
    // Two row lengths followed by one repeated row and one literal row.
    let encoded = [0, 2, 0, 4, 0xfe, 7, 2, 1, 2, 3];
    let abr = modern_abr(1, modern_block(2, 3, 8, 1, &encoded));

    let parsed = parse_abr(&abr).expect("compressed ABR");

    assert_eq!(parsed.brushes[0].mask, vec![7, 7, 7, 1, 2, 3]);
}

#[test]
fn parses_legacy_name_and_spacing() {
    let mut sample = Vec::new();
    sample.extend_from_slice(&0u32.to_be_bytes());
    sample.extend_from_slice(&15u16.to_be_bytes());
    sample.extend_from_slice(&4u32.to_be_bytes());
    for character in ['I' as u16, 'n' as u16, 'k' as u16, 0] {
        sample.extend_from_slice(&character.to_be_bytes());
    }
    sample.push(1);
    sample.extend_from_slice(&[0; 8]);
    add_bounds(&mut sample, 1, 2);
    sample.extend_from_slice(&8u16.to_be_bytes());
    sample.push(0);
    sample.extend_from_slice(&[0, 255]);

    let mut abr = Vec::new();
    abr.extend_from_slice(&2u16.to_be_bytes());
    abr.extend_from_slice(&1u16.to_be_bytes());
    abr.extend_from_slice(&2u16.to_be_bytes());
    abr.extend_from_slice(&(sample.len() as u32).to_be_bytes());
    abr.extend_from_slice(&sample);

    let parsed = parse_abr(&abr).expect("legacy ABR");
    assert_eq!(parsed.brushes[0].name.as_deref(), Some("Ink"));
    assert_eq!(parsed.brushes[0].spacing_percent, Some(15.0));
    assert_eq!(parsed.brushes[0].mask, vec![0, 255]);
}

#[test]
fn skips_bad_entry_when_another_brush_is_usable() {
    let mut section = modern_block(1, 1, 32, 0, &[0, 0, 0, 0]);
    section.extend_from_slice(&modern_block(1, 1, 8, 0, &[255]));
    let abr = modern_abr(1, section);

    let parsed = parse_abr(&abr).expect("partially valid ABR");

    assert_eq!(parsed.brushes.len(), 1);
    assert_eq!(parsed.warnings.len(), 1);
}

#[test]
fn rejects_truncated_and_unknown_files() {
    assert!(parse_abr(&[0, 6, 0]).is_err());
    assert!(parse_abr(&[0, 9, 0, 1]).is_err());
}

#[test]
fn rejects_cumulative_decoded_data_over_limit() {
    let mut parsed = ParsedAbr::default();
    let mut decoded_pixels = MAX_TOTAL_BRUSH_PIXELS;
    let brush = AbrBrush {
        name: None,
        width: 1,
        height: 1,
        mask: vec![255],
        spacing_percent: None,
    };

    let error = append_decoded_brush_within_budget(&mut parsed, &mut decoded_pixels, brush)
        .expect_err("decoded data over limit");

    assert!(error.to_string().contains("decoded brush data exceeds"));
    assert!(parsed.brushes.is_empty());
}

fn modern_abr(subversion: u16, section: Vec<u8>) -> Vec<u8> {
    modern_abr_version(6, subversion, section)
}

fn modern_abr_version(version: u16, subversion: u16, section: Vec<u8>) -> Vec<u8> {
    let mut abr = Vec::new();
    abr.extend_from_slice(&version.to_be_bytes());
    abr.extend_from_slice(&subversion.to_be_bytes());
    abr.extend_from_slice(b"8BIMsamp");
    abr.extend_from_slice(&(section.len() as u32).to_be_bytes());
    abr.extend_from_slice(&section);
    abr
}

fn modern_block(height: i32, width: i32, depth: u16, compression: u8, data: &[u8]) -> Vec<u8> {
    modern_block_with_prefix(47, height, width, depth, compression, data)
}

fn modern_block_with_prefix(
    prefix_size: usize,
    height: i32,
    width: i32,
    depth: u16,
    compression: u8,
    data: &[u8],
) -> Vec<u8> {
    let mut payload = vec![0; prefix_size];
    add_bounds(&mut payload, height, width);
    payload.extend_from_slice(&depth.to_be_bytes());
    payload.push(compression);
    payload.extend_from_slice(data);

    let mut block = Vec::new();
    block.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    block.extend_from_slice(&payload);
    while block.len() % 4 != 0 {
        block.push(0);
    }
    block
}

fn add_bounds(bytes: &mut Vec<u8>, height: i32, width: i32) {
    for value in [0i32, 0, height, width] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
}
