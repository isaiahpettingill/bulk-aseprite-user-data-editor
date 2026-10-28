//! Bounded, first-frame-only preview using the independent `asefile` renderer.
//! The original document is never rewritten. Only preview-relevant chunks are
//! copied into a temporary one-frame document after validation. In particular,
//! unknown/typed metadata cannot trigger allocations in the preview decoder.
use std::io::Read;

const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIMENSION: usize = 1024;
const MAX_PIXELS: usize = 2 * 1024 * 1024;
const MAX_CHUNKS: usize = 4096;
const MAX_LAYERS: usize = 128;
const MAX_PALETTE: usize = 4096;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewImage {
    pub width: usize,
    pub height: usize,
    /// Straight-alpha RGBA, row-major. Upload with nearest-neighbor sampling.
    pub rgba: Vec<u8>,
}

fn word(bytes: &[u8], at: usize) -> Result<usize> {
    let b = bytes.get(at..at + 2).ok_or("Truncated preview field")?;
    Ok(u16::from_le_bytes([b[0], b[1]]) as usize)
}
fn dword(bytes: &[u8], at: usize) -> Result<usize> {
    let b = bytes.get(at..at + 4).ok_or("Truncated preview field")?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()) as usize)
}
fn name_end(bytes: &[u8], at: usize) -> Result<usize> {
    let end = at
        .checked_add(2)
        .and_then(|p| p.checked_add(word(bytes, at).ok()?))
        .ok_or("Invalid preview name")?;
    if end > bytes.len() {
        return Err("Truncated preview name".into());
    }
    Ok(end)
}
fn bounded_zlib(bytes: &[u8], expected: usize) -> Result<()> {
    // `take` also bounds malformed streams whose declared cel dimensions lie.
    let decoder = flate2::read::ZlibDecoder::new(bytes);
    let mut output = Vec::new();
    decoder
        .take(expected as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|_| "Invalid compressed preview pixels")?;
    if output.len() != expected {
        return Err("Compressed pixels exceed or disagree with cel dimensions".into());
    }
    Ok(())
}
fn validate_palette(kind: usize, b: &[u8]) -> Result<()> {
    if kind == 0x2019 {
        let size = dword(b, 0)?;
        let first = dword(b, 4)?;
        let last = dword(b, 8)?;
        if size > MAX_PALETTE || first > last || last >= size {
            return Err("Preview palette limit or invalid range".into());
        }
        let mut p = 20usize;
        for _ in first..=last {
            let flags = word(b, p)?;
            p = p.checked_add(6).ok_or("Palette length overflow")?;
            if p > b.len() {
                return Err("Truncated preview palette".into());
            }
            if flags & 1 != 0 {
                p = name_end(b, p)?;
            }
        }
    } else {
        let packets = word(b, 0)?;
        if packets > 256 {
            return Err("Too many palette packets".into());
        }
        let mut p = 2;
        let mut index = 0;
        for _ in 0..packets {
            let pair = b.get(p..p + 2).ok_or("Truncated old palette")?;
            index += pair[0] as usize;
            let count = if pair[1] == 0 { 256 } else { pair[1] as usize };
            index += count;
            if index > 256 {
                return Err("Old palette range exceeds 256 colors".into());
            }
            p += 2 + count * 3;
            if p > b.len() {
                return Err("Truncated old palette colors".into());
            }
        }
    }
    Ok(())
}

/// Render frame zero only. Unsupported or oversized previews are ordinary
/// errors; the caller should continue allowing metadata editing.
pub fn render_first_frame(bytes: &[u8]) -> Result<PreviewImage> {
    if bytes.len() < 144 || bytes.len() > MAX_FILE_BYTES {
        return Err("Preview limited to files up to 16 MiB".into());
    }
    if dword(bytes, 0)? != bytes.len() || word(bytes, 4)? != 0xa5e0 || word(bytes, 6)? == 0 {
        return Err("Invalid preview document header".into());
    }
    let width = word(bytes, 8)?;
    let height = word(bytes, 10)?;
    let depth = word(bytes, 12)?;
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err("Preview limited to 1024 × 1024 pixels".into());
    }
    let bpp = match depth {
        8 => 1,
        16 => 2,
        32 => 4,
        _ => return Err("Unsupported preview color depth".into()),
    };
    let frame_size = dword(bytes, 128)?;
    if frame_size < 16 {
        return Err("Invalid preview frame size".into());
    }
    let end = 128usize
        .checked_add(frame_size)
        .ok_or("Frame size overflow")?;
    let frame = bytes.get(128..end).ok_or("Truncated preview frame")?;
    if word(frame, 4)? != 0xf1fa {
        return Err("Invalid preview frame magic".into());
    }
    let new_count = dword(frame, 12)?;
    let count = if new_count == 0 {
        word(frame, 6)?
    } else {
        new_count
    };
    if count > MAX_CHUNKS {
        return Err("Preview chunk limit exceeded".into());
    }
    let mut output = bytes[..128].to_vec();
    output[6..8].copy_from_slice(&1u16.to_le_bytes());
    let mut selected = frame[..16].to_vec();
    let mut emitted = 0u16;
    let mut layers = 0;
    let mut layer_levels: Vec<usize> = Vec::new();
    let mut layer_kinds: Vec<usize> = Vec::new();
    let mut pixel_budget = 0usize;
    let mut cursor = 16usize;
    for _ in 0..count {
        let size = dword(frame, cursor)?;
        let kind = word(frame, cursor + 4)?;
        if size < 6 {
            return Err("Invalid preview chunk size".into());
        }
        let next = cursor.checked_add(size).ok_or("Chunk size overflow")?;
        let chunk = frame.get(cursor..next).ok_or("Truncated preview chunk")?;
        let data = &chunk[6..];
        let keep = match kind {
            0x2004 => {
                layers += 1;
                if layers > MAX_LAYERS {
                    return Err("Preview limited to 128 layers".into());
                }
                let kind = word(data, 2)?;
                let depth = word(data, 4)?;
                if kind > 1 {
                    return Err("Tilemap previews are not supported".into());
                }
                if depth > MAX_LAYERS {
                    return Err("Invalid preview group depth".into());
                }
                if depth > 0 {
                    let parent = layer_levels
                        .iter()
                        .rposition(|level| *level < depth)
                        .ok_or("Preview layer is missing its parent")?;
                    if layer_levels[parent] + 1 != depth || layer_kinds[parent] != 1 {
                        return Err("Invalid preview layer hierarchy".into());
                    }
                }
                layer_levels.push(depth);
                layer_kinds.push(kind);
                name_end(data, 16)?;
                true
            }
            0x2005 => {
                if data.len() < 16 || word(data, 0)? >= layers {
                    return Err("Invalid preview cel layer".into());
                }
                if layer_kinds[word(data, 0)?] != 0 {
                    return Err("Preview cel belongs to a group layer".into());
                }
                let kind = word(data, 7)?;
                if !matches!(kind, 0 | 2) {
                    return Err("Linked/tilemap first-frame previews are not supported".into());
                }
                let w = word(data, 16)?;
                let h = word(data, 18)?;
                if w > MAX_DIMENSION || h > MAX_DIMENSION {
                    return Err("Preview cel dimension limit exceeded".into());
                }
                let pixels = w.checked_mul(h).ok_or("Cel size overflow")?;
                pixel_budget = pixel_budget
                    .checked_add(pixels)
                    .ok_or("Preview pixel budget overflow")?;
                if pixel_budget > MAX_PIXELS {
                    return Err("Preview decoded-pixel budget exceeded".into());
                }
                let expected = pixels * bpp;
                let content = &data[20..];
                if kind == 2 {
                    bounded_zlib(content, expected)?;
                } else if content.len() != expected {
                    return Err("Invalid raw preview pixel length".into());
                }
                true
            }
            4 | 0x11 | 0x2019 => {
                validate_palette(kind, data)?;
                true
            }
            // Tilesets and all metadata are irrelevant to ordinary image layers;
            // do not hand them to a library which may eagerly decode/allocate.
            _ => false,
        };
        if keep {
            selected.extend(chunk);
            emitted += 1;
        }
        cursor = next;
    }
    let frame_len = selected.len() as u32;
    selected[..4].copy_from_slice(&frame_len.to_le_bytes());
    selected[6..8].copy_from_slice(&emitted.to_le_bytes());
    selected[12..16].copy_from_slice(&(emitted as u32).to_le_bytes());
    output.extend(selected);
    let len = output.len() as u32;
    output[..4].copy_from_slice(&len.to_le_bytes());
    let sprite = asefile::AsepriteFile::read(output.as_slice())
        .map_err(|e| format!("Preview unavailable: {e}"))?;
    let image = sprite.frame(0).image();
    let rgba = image.into_raw();
    if rgba.len() != width * height * 4 {
        return Err("Unexpected preview dimensions".into());
    }
    Ok(PreviewImage {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture(compressed: bool, pixels: &[u8]) -> Vec<u8> {
        let mut header = vec![0; 128];
        header[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
        for p in [6, 8, 10] {
            header[p..p + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        header[12] = 32;
        let mut frame = vec![0; 16];
        frame[4..6].copy_from_slice(&0xf1fau16.to_le_bytes());
        frame[6] = 2;
        let mut layer = vec![0; 16];
        layer[0] = 1;
        layer[12] = 255;
        layer.extend([1, 0, b'L']);
        let mut cel = vec![0; 16];
        cel[6] = 255;
        if compressed {
            cel[7] = 2;
        }
        cel.extend([1, 0, 1, 0]);
        if compressed {
            let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
            z.write_all(pixels).unwrap();
            cel.extend(z.finish().unwrap());
        } else {
            cel.extend(pixels);
        }
        for (kind, body) in [(0x2004u16, layer), (0x2005, cel)] {
            frame.extend(((body.len() + 6) as u32).to_le_bytes());
            frame.extend(kind.to_le_bytes());
            frame.extend(body);
        }
        let len = frame.len() as u32;
        frame[..4].copy_from_slice(&len.to_le_bytes());
        header.extend(frame);
        let len = header.len() as u32;
        header[..4].copy_from_slice(&len.to_le_bytes());
        header
    }
    #[test]
    fn raw_and_compressed_render_match() {
        for compressed in [false, true] {
            let bytes = fixture(compressed, &[11, 40, 90, 255]);
            let original = bytes.clone();
            assert_eq!(
                render_first_frame(&bytes).unwrap(),
                PreviewImage {
                    width: 1,
                    height: 1,
                    rgba: vec![11, 40, 90, 255]
                }
            );
            assert_eq!(bytes, original);
        }
    }
    #[test]
    fn decompress_output_is_bounded() {
        let b = fixture(true, &vec![0; 100_000]);
        assert!(render_first_frame(&b).unwrap_err().contains("dimensions"));
    }
    #[test]
    fn truncated_inputs_are_errors() {
        let b = fixture(true, &[1, 2, 3, 255]);
        for end in 0..b.len() {
            assert!(render_first_frame(&b[..end]).is_err());
        }
    }
    #[test]
    fn oversized_dimensions_rejected_before_decode() {
        let mut b = fixture(true, &[1, 2, 3, 255]);
        b[8..10].copy_from_slice(&65535u16.to_le_bytes());
        assert!(render_first_frame(&b).unwrap_err().contains("1024"));
    }
    #[test]
    fn metadata_bytes_are_not_sent_to_renderer() {
        let mut b = fixture(false, &[11, 40, 90, 255]);
        let payload = vec![255; 32];
        b.extend(38u32.to_le_bytes());
        b.extend(0x2020u16.to_le_bytes());
        b.extend(payload);
        b[134] = 3;
        let len = b.len() as u32;
        b[..4].copy_from_slice(&len.to_le_bytes());
        b[128..132].copy_from_slice(&(len - 128).to_le_bytes());
        assert_eq!(render_first_frame(&b).unwrap().rgba, vec![11, 40, 90, 255]);
    }
    #[test]
    fn alpha_and_grayscale_are_preserved() {
        let rgba = render_first_frame(&fixture(false, &[11, 40, 90, 128])).unwrap();
        assert_eq!(rgba.rgba, vec![11, 40, 90, 128]);
        let mut grayscale = fixture(false, &[90, 255]);
        grayscale[12] = 16;
        assert_eq!(
            render_first_frame(&grayscale).unwrap().rgba,
            vec![90, 90, 90, 255]
        );
    }
    #[test]
    fn indexed_palette_renders_through_library() {
        let mut bytes = fixture(false, &[1]);
        bytes[12] = 8;
        let mut palette = vec![];
        palette.extend(2u32.to_le_bytes());
        palette.extend(0u32.to_le_bytes());
        palette.extend(1u32.to_le_bytes());
        palette.extend([0; 8]);
        palette.extend([0, 0, 0, 0, 0, 0]);
        palette.extend([0, 0, 11, 40, 90, 255]);
        bytes.extend(((palette.len() + 6) as u32).to_le_bytes());
        bytes.extend(0x2019u16.to_le_bytes());
        bytes.extend(palette);
        bytes[134] = 3;
        let len = bytes.len() as u32;
        bytes[..4].copy_from_slice(&len.to_le_bytes());
        bytes[128..132].copy_from_slice(&(len - 128).to_le_bytes());
        assert_eq!(
            render_first_frame(&bytes).unwrap().rgba,
            vec![11, 40, 90, 255]
        );
    }
    #[test]
    fn later_frames_are_not_decoded() {
        let mut bytes = fixture(false, &[11, 40, 90, 255]);
        bytes[6] = 2;
        let mut later = vec![0; 16];
        later[..4].copy_from_slice(&16u32.to_le_bytes());
        later[4..6].copy_from_slice(&0xf1fau16.to_le_bytes());
        bytes.extend(later);
        let len = bytes.len() as u32;
        bytes[..4].copy_from_slice(&len.to_le_bytes());
        assert_eq!(
            render_first_frame(&bytes).unwrap().rgba,
            vec![11, 40, 90, 255]
        );
    }
    #[test]
    fn palette_and_chunk_count_limits_fail_safely() {
        let mut bytes = fixture(false, &[1, 2, 3, 255]);
        bytes[140..144].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(render_first_frame(&bytes).is_err());
        let mut palette = vec![0; 20];
        palette[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(validate_palette(0x2019, &palette).is_err());
    }

    #[test]
    fn invalid_group_parent_is_rejected_before_library() {
        let mut bytes = fixture(false, &[1, 2, 3, 255]);
        bytes[154..156].copy_from_slice(&1u16.to_le_bytes());
        assert!(render_first_frame(&bytes).unwrap_err().contains("parent"));
    }
}
