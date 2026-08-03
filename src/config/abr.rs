use std::{error::Error, fmt};

const MAX_BRUSH_COUNT: usize = 2_048;
const MAX_BRUSH_DIMENSION: u32 = 16_384;
const MAX_BRUSH_PIXELS: usize = 64 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) struct AbrBrush {
    pub(crate) name: Option<String>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// One byte of coverage per pixel, from transparent (0) to opaque (255).
    pub(crate) mask: Vec<u8>,
    /// ABR spacing is stored as a percentage of the brush diameter.
    pub(crate) spacing_percent: Option<f32>,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ParsedAbr {
    pub(crate) brushes: Vec<AbrBrush>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AbrError {
    message: String,
}

impl AbrError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AbrError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AbrError {}

pub(crate) fn parse_abr(bytes: &[u8]) -> Result<ParsedAbr, AbrError> {
    let mut input = Reader::new(bytes);
    let version = input.read_u16()?;
    let count_or_subversion = input.read_u16()?;

    match version {
        1 | 2 => parse_legacy(&mut input, version, count_or_subversion as usize),
        6 | 7 | 9 | 10 if matches!(count_or_subversion, 1 | 2) => {
            parse_tagged(&mut input, count_or_subversion)
        }
        6 | 7 | 9 | 10 => Err(AbrError::new(format!(
            "unsupported ABR version {version}.{count_or_subversion}"
        ))),
        _ => Err(AbrError::new(format!("unsupported ABR version {version}"))),
    }
}

fn parse_legacy(
    input: &mut Reader<'_>,
    version: u16,
    brush_count: usize,
) -> Result<ParsedAbr, AbrError> {
    if brush_count > MAX_BRUSH_COUNT {
        return Err(AbrError::new(format!(
            "ABR contains too many brushes ({brush_count})"
        )));
    }

    let mut parsed = ParsedAbr::default();
    for index in 0..brush_count {
        let brush_type = input.read_u16()?;
        let block_size = input.read_u32()? as usize;
        let mut block = input.take(block_size)?;
        if brush_type != 2 {
            parsed.warnings.push(format!(
                "Brush {} uses an unsupported computed tip",
                index + 1
            ));
            continue;
        }

        match parse_legacy_sample(&mut block, version) {
            Ok(brush) => parsed.brushes.push(brush),
            Err(error) => parsed
                .warnings
                .push(format!("Brush {} could not be decoded: {error}", index + 1)),
        }
    }
    ensure_has_brushes(parsed)
}

fn parse_legacy_sample(input: &mut Reader<'_>, version: u16) -> Result<AbrBrush, AbrError> {
    input.skip(4)?; // legacy miscellaneous field
    let spacing = input.read_u16()?;
    let name = if version == 2 {
        read_ucs2_name(input)?
    } else {
        None
    };
    input.skip(1)?; // antialiasing flag
    input.skip(8)?; // obsolete 16-bit bounds
    let top = input.read_i32()?;
    let left = input.read_i32()?;
    let bottom = input.read_i32()?;
    let right = input.read_i32()?;
    let depth = input.read_u16()?;
    let (width, height, pixel_count) = checked_dimensions(top, left, bottom, right)?;
    if depth != 8 {
        return Err(AbrError::new(format!(
            "unsupported legacy brush depth {depth}"
        )));
    }
    let compression = input.read_u8()?;
    let mask = decode_mask(input, width as usize, height as usize, 1, compression)?;

    debug_assert_eq!(mask.len(), pixel_count);
    Ok(AbrBrush {
        name,
        width,
        height,
        mask,
        spacing_percent: Some(spacing as f32),
    })
}

fn parse_tagged(input: &mut Reader<'_>, subversion: u16) -> Result<ParsedAbr, AbrError> {
    let mut parsed = None;
    while input.remaining() >= 12 {
        let signature = input.read_array::<4>()?;
        if &signature != b"8BIM" {
            return Err(AbrError::new("invalid ABR section signature"));
        }
        let tag = input.read_array::<4>()?;
        let section_size = input.read_u32()? as usize;
        let mut section = input.take(section_size)?;
        if &tag == b"samp" {
            parsed = Some(parse_sample_section(&mut section, subversion)?);
        }
        let padding = (4 - section_size % 4) % 4;
        if input.remaining() >= padding {
            input.skip(padding)?;
        } else if input.remaining() != 0 {
            return Err(AbrError::new("truncated ABR section padding"));
        }
    }
    if input.remaining() != 0 {
        return Err(AbrError::new("truncated ABR section header"));
    }

    parsed.ok_or_else(|| AbrError::new("ABR does not contain a sampled-brush section"))
}

fn parse_sample_section(section: &mut Reader<'_>, subversion: u16) -> Result<ParsedAbr, AbrError> {
    let mut parsed = ParsedAbr::default();
    let mut index = 0usize;
    while section.remaining() >= 4 {
        if index >= MAX_BRUSH_COUNT {
            return Err(AbrError::new(format!(
                "ABR contains more than {MAX_BRUSH_COUNT} brushes"
            )));
        }
        index += 1;

        let block_size = section.read_u32()? as usize;
        if block_size == 0 {
            return Err(AbrError::new("ABR contains an empty brush block"));
        }
        let mut block = section.take(block_size)?;
        match parse_tagged_sample(&mut block, subversion) {
            Ok(brush) => parsed.brushes.push(brush),
            Err(error) => parsed
                .warnings
                .push(format!("Brush {index} could not be decoded: {error}")),
        }

        let padding = (4 - (block_size % 4)) % 4;
        section.skip(padding)?;
    }
    if section.remaining() != 0 {
        return Err(AbrError::new("truncated ABR brush block header"));
    }
    ensure_has_brushes(parsed)
}

fn parse_tagged_sample(input: &mut Reader<'_>, subversion: u16) -> Result<AbrBrush, AbrError> {
    input.skip(if subversion == 1 { 47 } else { 301 })?;
    let top = input.read_i32()?;
    let left = input.read_i32()?;
    let bottom = input.read_i32()?;
    let right = input.read_i32()?;
    let depth_bits = input.read_u16()?;
    let bytes_per_pixel = match depth_bits {
        8 => 1,
        16 => 2,
        _ => {
            return Err(AbrError::new(format!(
                "unsupported brush depth {depth_bits}"
            )));
        }
    };
    let compression = input.read_u8()?;
    if compression == 1 && bytes_per_pixel != 1 {
        return Err(AbrError::new(
            "compressed 16-bit ABR brush tips are unsupported",
        ));
    }
    let (width, height, _) = checked_dimensions(top, left, bottom, right)?;
    let mask = decode_mask(
        input,
        width as usize,
        height as usize,
        bytes_per_pixel,
        compression,
    )?;

    Ok(AbrBrush {
        name: None,
        width,
        height,
        mask,
        spacing_percent: None,
    })
}

fn decode_mask(
    input: &mut Reader<'_>,
    width: usize,
    height: usize,
    bytes_per_pixel: usize,
    compression: u8,
) -> Result<Vec<u8>, AbrError> {
    let sample_count = width
        .checked_mul(height)
        .ok_or_else(|| AbrError::new("brush dimensions overflow"))?;
    let byte_count = sample_count
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| AbrError::new("brush data size overflows"))?;

    match (compression, bytes_per_pixel) {
        (0, 1) => Ok(input.read_bytes(byte_count)?.to_vec()),
        (0, 2) => {
            let source = input.read_bytes(byte_count)?;
            Ok(source
                .chunks_exact(2)
                // Version 6/10 sampled tips store their 16-bit values little-endian.
                .map(|sample| (u16::from_le_bytes([sample[0], sample[1]]) >> 8) as u8)
                .collect())
        }
        (1, 1) => decode_packbits_rows(input, width, height),
        (method, _) => Err(AbrError::new(format!(
            "unsupported ABR compression method {method}"
        ))),
    }
}

fn decode_packbits_rows(
    input: &mut Reader<'_>,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, AbrError> {
    let mut row_lengths = Vec::with_capacity(height);
    for _ in 0..height {
        let length = input.read_u16()? as usize;
        if length == 0 {
            return Err(AbrError::new("ABR contains an empty compressed row"));
        }
        row_lengths.push(length);
    }

    let mut output = Vec::with_capacity(width * height);
    for length in row_lengths {
        let encoded = input.read_bytes(length)?;
        let row_start = output.len();
        let mut position = 0usize;
        while position < encoded.len() {
            let control = encoded[position] as i8;
            position += 1;
            match control {
                -128 => {}
                -127..=-1 => {
                    let value = *encoded
                        .get(position)
                        .ok_or_else(|| AbrError::new("truncated PackBits run"))?;
                    position += 1;
                    let count = (1i16 - control as i16) as usize;
                    if output.len() - row_start + count > width {
                        return Err(AbrError::new("PackBits row exceeds brush width"));
                    }
                    output.resize(output.len() + count, value);
                }
                0..=127 => {
                    let count = control as usize + 1;
                    let end = position
                        .checked_add(count)
                        .ok_or_else(|| AbrError::new("PackBits run length overflows"))?;
                    let values = encoded
                        .get(position..end)
                        .ok_or_else(|| AbrError::new("truncated PackBits literal"))?;
                    if output.len() - row_start + count > width {
                        return Err(AbrError::new("PackBits row exceeds brush width"));
                    }
                    output.extend_from_slice(values);
                    position = end;
                }
            }
        }
        if output.len() - row_start != width {
            return Err(AbrError::new("PackBits row does not fill brush width"));
        }
    }
    Ok(output)
}

fn checked_dimensions(
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
) -> Result<(u32, u32, usize), AbrError> {
    let width = right
        .checked_sub(left)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0 && *value <= MAX_BRUSH_DIMENSION)
        .ok_or_else(|| AbrError::new("brush width is out of range"))?;
    let height = bottom
        .checked_sub(top)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0 && *value <= MAX_BRUSH_DIMENSION)
        .ok_or_else(|| AbrError::new("brush height is out of range"))?;
    let pixels = width as usize * height as usize;
    if pixels > MAX_BRUSH_PIXELS {
        return Err(AbrError::new("brush tip contains too many pixels"));
    }
    Ok((width, height, pixels))
}

fn read_ucs2_name(input: &mut Reader<'_>) -> Result<Option<String>, AbrError> {
    let character_count = input.read_u32()? as usize;
    if character_count == 0 || character_count > 16_384 {
        return Err(AbrError::new("legacy brush name length is out of range"));
    }
    let mut characters = Vec::with_capacity(character_count);
    for _ in 0..character_count {
        characters.push(input.read_u16()?);
    }
    while characters.last() == Some(&0) {
        characters.pop();
    }
    let name = String::from_utf16_lossy(&characters);
    let name = name.trim();
    Ok((!name.is_empty()).then(|| name.to_owned()))
}

fn ensure_has_brushes(parsed: ParsedAbr) -> Result<ParsedAbr, AbrError> {
    if parsed.brushes.is_empty() {
        let details = parsed
            .warnings
            .first()
            .map_or("no sampled brushes were found", String::as_str);
        Err(AbrError::new(format!(
            "ABR does not contain a usable brush: {details}"
        )))
    } else {
        Ok(parsed)
    }
}

#[derive(Clone, Copy)]
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn read_u8(&mut self) -> Result<u8, AbrError> {
        Ok(self.read_array::<1>()?[0])
    }

    fn read_u16(&mut self) -> Result<u16, AbrError> {
        Ok(u16::from_be_bytes(self.read_array()?))
    }

    fn read_u32(&mut self) -> Result<u32, AbrError> {
        Ok(u32::from_be_bytes(self.read_array()?))
    }

    fn read_i32(&mut self) -> Result<i32, AbrError> {
        Ok(i32::from_be_bytes(self.read_array()?))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], AbrError> {
        self.read_bytes(N)?
            .try_into()
            .map_err(|_| AbrError::new("truncated ABR file"))
    }

    fn read_bytes(&mut self, count: usize) -> Result<&'a [u8], AbrError> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| AbrError::new("truncated ABR file"))?;
        let bytes = &self.bytes[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn skip(&mut self, count: usize) -> Result<(), AbrError> {
        self.read_bytes(count).map(|_| ())
    }

    fn take(&mut self, count: usize) -> Result<Reader<'a>, AbrError> {
        self.read_bytes(count).map(Reader::new)
    }
}

#[cfg(test)]
mod tests {
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
        let abr = modern_abr(2, modern_block_with_prefix(301, 1, 1, 16, 0, &[0, 128]));

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
}
