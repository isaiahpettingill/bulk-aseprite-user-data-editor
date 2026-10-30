//! Transactional artwork edits. Unknown metadata chunks are retained verbatim.
//! Binary layout: https://github.com/aseprite/aseprite/blob/main/docs/ase-file-specs.md
use super::*;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug)]
pub enum ArtworkOperation {
    FlipHorizontal,
    ExpandCanvas { right: u16, bottom: u16 },
    ConvertToIndexed,
    ConvertToRgba,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Color {
    pub(super) rgba: [u8; 4],
    pub(super) name: Option<String>,
}
pub(super) type Palette = Vec<Option<Color>>;

impl Document {
    pub fn dimensions(&self) -> (u16, u16) {
        (
            u16at(&self.header, 8).unwrap(),
            u16at(&self.header, 10).unwrap(),
        )
    }

    /// An error leaves the entire document unchanged, including its metadata.
    pub fn transform(&mut self, operation: ArtworkOperation) -> Result<()> {
        let mut next = self.clone();
        match operation {
            ArtworkOperation::ExpandCanvas { right, bottom } => {
                let (width, height) = self.dimensions();
                let width = width
                    .checked_add(right)
                    .ok_or("Expanded width exceeds 65535 pixels")?;
                let height = height
                    .checked_add(bottom)
                    .ok_or("Expanded height exceeds 65535 pixels")?;
                next.header[8..10].copy_from_slice(&width.to_le_bytes());
                next.header[10..12].copy_from_slice(&height.to_le_bytes());
            }
            ArtworkOperation::FlipHorizontal => next.flip()?,
            ArtworkOperation::ConvertToIndexed => next.convert(8)?,
            ArtworkOperation::ConvertToRgba => next.convert(32)?,
        }
        // Re-index metadata associations against the new chunk layout.
        *self = Self::parse(&next.encode()?)?;
        Ok(())
    }

    pub(super) fn image_layers(&self) -> Result<Vec<u16>> {
        let mut flags = Vec::new();
        for (fi, frame) in self.frames.iter().enumerate() {
            for chunk in &frame.chunks {
                match chunk.kind {
                    0x2004 => {
                        if fi != 0 {
                            return Err(
                                "Layer definitions outside the first frame are unsupported".into(),
                            );
                        }
                        let layer_flags = u16at(&chunk.data, 0)?;
                        if u16at(&chunk.data, 2)? > 1 || layer_flags & 64 != 0 {
                            return Err("Tilemap and reference layers are not supported for artwork transforms".into());
                        }
                        flags.push(layer_flags);
                    }
                    0x2023 => {
                        return Err("Tilesets are not supported for artwork transforms".into())
                    }
                    _ => (),
                }
            }
        }
        Ok(flags)
    }

    fn cel_at(&self, frame: usize, layer: u16) -> Result<&Chunk> {
        let frame = self
            .frames
            .get(frame)
            .ok_or("Linked cel references a missing frame")?;
        let mut found = None;
        for chunk in &frame.chunks {
            if chunk.kind == 0x2005 && u16at(&chunk.data, 0)? == layer {
                if found.is_some() {
                    return Err("Duplicate cels in one frame/layer".into());
                }
                found = Some(chunk);
            }
        }
        found.ok_or_else(|| "Linked cel references a missing cel".into())
    }

    pub(super) fn source_cel(&self, frame: usize, layer: u16) -> Result<&Chunk> {
        let mut frame = frame;
        let mut visited = std::collections::BTreeSet::new();
        loop {
            if !visited.insert(frame) {
                return Err("Cyclic linked cels".into());
            }
            let cel = self.cel_at(frame, layer)?;
            match u16at(&cel.data, 7)? {
                0 | 2 => return Ok(cel),
                1 => {
                    if cel.data.len() != 18 {
                        return Err("Malformed linked cel".into());
                    }
                    frame = u16at(&cel.data, 16)? as usize;
                }
                _ => return Err("Tilemap or unknown cel encoding is unsupported".into()),
            }
        }
    }

    fn flip(&mut self) -> Result<()> {
        let layers = self.image_layers()?;
        let depth = u16at(&self.header, 12)? as usize / 8;
        let canvas_width = i32::from(self.dimensions().0);
        let mut replacements = Vec::new();
        let mut budget = MAX_FILE;
        for (fi, frame) in self.frames.iter().enumerate() {
            for (ci, chunk) in frame.chunks.iter().enumerate() {
                if chunk.kind == 0x2006 && u32at(&chunk.data, 0)? & 1 != 0 {
                    return Err("Precise/reference cel bounds cannot be mirrored safely".into());
                }
                if chunk.kind != 0x2005 {
                    continue;
                }
                let layer = u16at(&chunk.data, 0)?;
                if layer as usize >= layers.len() {
                    return Err("Cel references a missing layer".into());
                }
                let source = self.source_cel(fi, layer)?;
                let width = u16at(&source.data, 16)?;
                let x = i32::from(u16at(&chunk.data, 2)? as i16);
                let mirrored =
                    i16::try_from(canvas_width - x - i32::from(width)).map_err(|_| {
                        "Mirrored cel position exceeds Aseprite's signed coordinate range"
                    })?;
                let mut data = chunk.data.clone();
                data[2..4].copy_from_slice(&mirrored.to_le_bytes());
                if u16at(&data, 7)? != 1 {
                    let (width, height, mut pixels) = decode_cel(&data, depth, &mut budget)?;
                    for row in pixels.chunks_exact_mut(width as usize * depth) {
                        for x in 0..width as usize / 2 {
                            for channel in 0..depth {
                                row.swap(
                                    x * depth + channel,
                                    (width as usize - 1 - x) * depth + channel,
                                );
                            }
                        }
                    }
                    data = encode_cel(&data, width, height, &pixels)?;
                }
                replacements.push((fi, ci, data));
            }
        }
        for (fi, ci, data) in replacements {
            self.frames[fi].chunks[ci].data = data;
        }
        Ok(())
    }

    pub(super) fn palettes(&self) -> Result<Vec<Palette>> {
        let mut palette = Vec::new();
        let mut frames = Vec::new();
        let mut palette_budget = MAX_FILE / 64;
        for frame in &self.frames {
            // New palettes supersede legacy compatibility chunks in the same frame.
            let modern = frame.chunks.iter().any(|c| c.kind == 0x2019);
            for chunk in &frame.chunks {
                if chunk.kind == 0x2019 {
                    read_palette(&chunk.data, &mut palette)?;
                } else if !modern && matches!(chunk.kind, 0x0004 | 0x0011) {
                    read_old_palette(&chunk.data, chunk.kind == 0x0011, &mut palette)?;
                }
            }
            palette_budget = palette_budget
                .checked_sub(palette.len())
                .ok_or("Animated palettes exceed the operation memory limit")?;
            frames.push(palette.clone());
        }
        Ok(frames)
    }

    fn convert(&mut self, target: u16) -> Result<()> {
        let depth = u16at(&self.header, 12)?;
        if depth == target {
            return Ok(());
        }
        let layers = self.image_layers()?;
        let palettes = self.palettes()?;
        let image_layer_count = self
            .frames
            .iter()
            .flat_map(|f| &f.chunks)
            .filter(|c| c.kind == 0x2004 && u16at(&c.data, 2).ok() == Some(0))
            .count();
        // Indexed->indexed compositing overwrites indices (blend_internals.h),
        // unlike indexed->RGBA rendering. Only single image-layer sprites can
        // preserve arbitrary palette alpha across both paths.
        // Indexed rendering does not preserve RGBA compositing in these cases.
        if target == 8 || depth == 8 {
            for frame in &self.frames {
                for chunk in &frame.chunks {
                    if chunk.kind == 0x2004
                        && (u16at(&chunk.data, 10)? != 0
                            || (u32at(&self.header, 14)? & 1 != 0 && chunk.data[12] != 255))
                    {
                        return Err("Indexed conversion requires normal-blend, fully opaque layers; compositing would change".into());
                    }
                    if chunk.kind == 0x2005 && chunk.data[6] != 255 {
                        return Err("Indexed conversion cannot preserve cel opacity".into());
                    }
                }
            }
        }
        let transparent = self.header[28] as usize;
        let mut output_palettes = Vec::new();
        let mut replacements = Vec::new();
        let mut budget = MAX_FILE;
        // A single transparent slot must be valid in every output frame. Reserve
        // a new slot rather than overwriting any original palette entry.
        let (output_transparent, transparent_colors) = if target == 8 {
            let mut needed = Vec::new();
            let mut transparent_colors = Vec::new();
            let mut scan_budget = MAX_FILE;
            for (fi, frame) in self.frames.iter().enumerate() {
                let mut colors = std::collections::BTreeSet::new();
                let mut transparent_color: Option<[u8; 4]> = None;
                for chunk in &frame.chunks {
                    if chunk.kind != 0x2005 {
                        continue;
                    }
                    let layer = u16at(&chunk.data, 0)? as usize;
                    if *layers.get(layer).ok_or("Cel references a missing layer")? & 8 != 0 {
                        continue;
                    }
                    let source = self.source_cel(fi, layer as u16)?;
                    let (_, _, pixels) =
                        decode_cel(&source.data, depth as usize / 8, &mut scan_budget)?;
                    for p in pixels.chunks_exact(depth as usize / 8) {
                        let rgba = if depth == 32 {
                            [p[0], p[1], p[2], p[3]]
                        } else {
                            [p[0], p[0], p[0], p[1]]
                        };
                        if rgba[3] != 0 && rgba[3] != 255 && image_layer_count != 1 {
                            return Err("Partial alpha on multiple layers cannot be converted without changing indexed compositing".into());
                        }
                        if rgba[3] == 0 {
                            if image_layer_count != 1
                                && transparent_color.is_some_and(|c| c != rgba)
                            {
                                return Err("Multiple hidden RGB colors in transparent pixels on multiple layers cannot be preserved in one indexed transparent slot".into());
                            }
                            transparent_color.get_or_insert(rgba);
                        } else {
                            colors.insert(rgba);
                        }
                    }
                }
                needed.push(colors);
                transparent_colors.push(transparent_color);
            }
            let max_size = palettes.iter().map(Vec::len).max().unwrap_or(0);
            if max_size > 256 {
                return Err("Existing palette exceeds indexed mode's 256-entry limit".into());
            }
            // Reuse a slot only if its exact color can represent every frame's
            // transparent pixels. Otherwise append; never overwrite palette colors.
            let index = (0..max_size)
                .find(|&i| {
                    palettes.iter().zip(&transparent_colors).zip(&needed).all(
                        |((p, transparent), needed)| {
                            p.get(i)
                                .and_then(Option::as_ref)
                                .is_some_and(|c| match transparent {
                                    Some(rgba) => c.rgba == *rgba,
                                    None => {
                                        !needed.contains(&c.rgba)
                                            || p.iter().enumerate().any(|(j, other)| {
                                                j != i
                                                    && other
                                                        .as_ref()
                                                        .is_some_and(|other| other.rgba == c.rgba)
                                            })
                                    }
                                })
                        },
                    )
                })
                .or_else(|| (max_size < 256).then_some(max_size))
                .ok_or("No free transparent palette slot (indexed mode allows 256 entries)")?;
            (index, transparent_colors)
        } else {
            (transparent, Vec::new())
        };
        for (fi, frame) in self.frames.iter().enumerate() {
            let mut palette = palettes[fi].clone();
            if target == 8 {
                if palette.len() <= output_transparent {
                    palette.resize(output_transparent + 1, None);
                }
                if palette[output_transparent].is_none() {
                    palette[output_transparent] = Some(Color {
                        rgba: transparent_colors[fi].unwrap_or([0; 4]),
                        name: None,
                    });
                }
            }
            for (ci, chunk) in frame.chunks.iter().enumerate() {
                if chunk.kind != 0x2005 {
                    continue;
                }
                let layer = u16at(&chunk.data, 0)? as usize;
                let background =
                    *layers.get(layer).ok_or("Cel references a missing layer")? & 8 != 0;
                let source = self.source_cel(fi, layer as u16)?;
                let (width, height, pixels) =
                    decode_cel(&source.data, depth as usize / 8, &mut budget)?;
                if target > depth {
                    let extra =
                        (width as usize) * (height as usize) * ((target - depth) as usize / 8);
                    budget = budget
                        .checked_sub(extra)
                        .ok_or("Converted artwork exceeds the 256 MiB operation limit")?;
                }
                let mut converted = Vec::new();
                for pixel in pixels.chunks_exact(depth as usize / 8) {
                    let rgba = match depth {
                        32 => [pixel[0], pixel[1], pixel[2], pixel[3]],
                        16 => [pixel[0], pixel[0], pixel[0], pixel[1]],
                        8 => {
                            let index = pixel[0] as usize;
                            let mut color = palettes[fi]
                                .get(index)
                                .and_then(Option::as_ref)
                                .ok_or("Indexed pixel references an undefined palette color")?
                                .rgba;
                            if !background && index == transparent {
                                color[3] = 0;
                            } else if image_layer_count != 1 && color[3] != 255 {
                                // Indexed compositing replaces a destination index,
                                // even when that palette entry has partial/zero alpha.
                                // RGBA compositing blends instead, so conversion could
                                // reveal underlying layers or change their colors.
                                return Err("Palette alpha on multiple layers cannot be converted to RGBA without changing indexed compositing".into());
                            }
                            color
                        }
                        _ => unreachable!(),
                    };
                    if target == 32 {
                        converted.extend(rgba);
                    } else {
                        if background && rgba[3] != 255 {
                            return Err("Nonopaque background pixels cannot be converted losslessly to indexed".into());
                        }
                        if !background && rgba[3] == 0 && transparent_colors[fi] == Some(rgba) {
                            converted.push(output_transparent as u8);
                            continue;
                        }
                        let index = palette.iter().enumerate().position(|(i, c)| {
                            (background || i != output_transparent || rgba[3] == 0)
                                && c.as_ref().is_some_and(|c| c.rgba == rgba)
                        });
                        let index = if let Some(index) = index {
                            index
                        } else {
                            if palette.len() >= 256 {
                                return Err("Exact colors plus existing palette exceed 256 entries; file skipped without quantization".into());
                            }
                            palette.push(Some(Color { rgba, name: None }));
                            palette.len() - 1
                        };
                        converted.push(index as u8);
                    }
                }
                // Materialize linked cels: the frame's palette can differ from the
                // source frame. Retain the link's own placement/opacity/z-index.
                replacements.push((fi, ci, encode_cel(&chunk.data, width, height, &converted)?));
            }
            output_palettes.push(palette);
        }
        for (fi, ci, data) in replacements {
            self.frames[fi].chunks[ci].data = data;
        }
        if target == 8 {
            self.header[28] = output_transparent as u8;
            let size = output_palettes.iter().map(Vec::len).max().unwrap_or(1) as u16;
            self.header[32..34].copy_from_slice(&size.to_le_bytes());
            for (frame, palette) in self.frames.iter_mut().zip(output_palettes) {
                // Append after existing metadata, so no existing user-data owner
                // changes. Original palettes and their named entries stay intact.
                frame.chunks.push(Chunk {
                    uid: self.next_uid,
                    kind: 0x2019,
                    data: write_palette(&palette)?,
                });
                self.next_uid += 1;
            }
        }
        self.header[12..14].copy_from_slice(&target.to_le_bytes());
        Ok(())
    }
}

pub(super) fn decode_cel(
    data: &[u8],
    bpp: usize,
    budget: &mut usize,
) -> Result<(u16, u16, Vec<u8>)> {
    let width = u16at(data, 16)?;
    let height = u16at(data, 18)?;
    if width == 0 || height == 0 {
        return Err("Zero-sized cel is unsupported".into());
    }
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(bpp))
        .ok_or("Cel size overflow")?;
    *budget = budget
        .checked_sub(len)
        .ok_or("Decoded artwork exceeds the 256 MiB operation limit")?;
    let payload = data.get(20..).ok_or("Truncated cel")?;
    let pixels = match u16at(data, 7)? {
        0 => payload.to_vec(),
        2 => {
            let mut decoder = ZlibDecoder::new(payload);
            let mut pixels = Vec::new();
            decoder
                .by_ref()
                .take(len as u64 + 1)
                .read_to_end(&mut pixels)
                .map_err(|e| format!("Invalid compressed cel: {e}"))?;
            if decoder.total_in() as usize != payload.len() {
                return Err("Trailing or incomplete compressed cel data".into());
            }
            pixels
        }
        _ => return Err("Unsupported cel encoding".into()),
    };
    if pixels.len() != len {
        return Err("Cel pixel length does not match its dimensions".into());
    }
    Ok((width, height, pixels))
}

fn encode_cel(original: &[u8], width: u16, height: u16, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut data = original.get(..16).ok_or("Truncated cel header")?.to_vec();
    data[7..9].copy_from_slice(&2u16.to_le_bytes());
    data.extend(width.to_le_bytes());
    data.extend(height.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(pixels).map_err(|e| e.to_string())?;
    data.extend(encoder.finish().map_err(|e| e.to_string())?);
    Ok(data)
}

fn read_palette(data: &[u8], palette: &mut Palette) -> Result<()> {
    let mut r = Reader::new(data);
    let size = r.u32()? as usize;
    let first = r.u32()? as usize;
    let last = r.u32()? as usize;
    if size == 0 || size > 65536 || first > last || last >= size {
        return Err("Invalid palette range".into());
    }
    r.take(8)?;
    palette.resize(size, None);
    for entry in &mut palette[first..=last] {
        let flags = r.u16()?;
        if flags & !1 != 0 {
            return Err("Unknown palette entry flags".into());
        }
        let rgba = r.take(4)?.try_into().unwrap();
        let name = if flags & 1 != 0 {
            Some(r.string()?)
        } else {
            None
        };
        *entry = Some(Color { rgba, name });
    }
    if r.pos != data.len() {
        return Err("Trailing palette data".into());
    }
    Ok(())
}
fn read_old_palette(data: &[u8], six_bit: bool, palette: &mut Palette) -> Result<()> {
    let mut r = Reader::new(data);
    let count = r.u16()?;
    let mut index = 0usize;
    for _ in 0..count {
        index += r.u8()? as usize;
        let count = match r.u8()? {
            0 => 256,
            n => n as usize,
        };
        if index + count > 256 {
            return Err("Legacy palette exceeds 256 colors".into());
        }
        if palette.len() < index + count {
            palette.resize(index + count, None);
        }
        for entry in &mut palette[index..index + count] {
            let rgb = r.take(3)?;
            let mut rgba = [0, 0, 0, 255];
            for channel in 0..3 {
                if six_bit && rgb[channel] > 63 {
                    return Err("Invalid six-bit legacy palette color".into());
                }
                rgba[channel] = if six_bit {
                    (rgb[channel] << 2) | (rgb[channel] >> 4)
                } else {
                    rgb[channel]
                };
            }
            *entry = Some(Color { rgba, name: None });
        }
        index += count;
    }
    if r.pos != data.len() {
        return Err("Trailing legacy palette data".into());
    }
    Ok(())
}
pub(super) fn write_palette(palette: &Palette) -> Result<Vec<u8>> {
    let mut data = (palette.len() as u32).to_le_bytes().to_vec();
    data.extend(0u32.to_le_bytes());
    data.extend((palette.len() as u32 - 1).to_le_bytes());
    data.extend([0; 8]);
    for entry in palette {
        let color = entry
            .as_ref()
            .ok_or("Incomplete palette: undefined entries cannot be replaced losslessly")?;
        data.extend(u16::from(color.name.is_some()).to_le_bytes());
        data.extend(color.rgba);
        if let Some(name) = &color.name {
            strout(&mut data, name)?;
        }
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(depth: u16, frames: Vec<Vec<(u16, Vec<u8>)>>) -> Document {
        let mut bytes = vec![0u8; 128];
        bytes[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&(frames.len() as u16).to_le_bytes());
        bytes[8..10].copy_from_slice(&8u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&3u16.to_le_bytes());
        bytes[12..14].copy_from_slice(&depth.to_le_bytes());
        bytes[14] = 1;
        for chunks in frames {
            let mut frame = vec![0u8; 16];
            frame[4..6].copy_from_slice(&0xf1fau16.to_le_bytes());
            frame[6..8].copy_from_slice(&(chunks.len() as u16).to_le_bytes());
            frame[8..10].copy_from_slice(&100u16.to_le_bytes());
            for (kind, data) in chunks {
                frame.extend(((data.len() + 6) as u32).to_le_bytes());
                frame.extend(kind.to_le_bytes());
                frame.extend(data);
            }
            let len = frame.len() as u32;
            frame[..4].copy_from_slice(&len.to_le_bytes());
            bytes.extend(frame);
        }
        let len = bytes.len() as u32;
        bytes[..4].copy_from_slice(&len.to_le_bytes());
        Document::parse(&bytes).unwrap()
    }
    fn layer(background: bool) -> Vec<u8> {
        let mut data = vec![0; 16];
        data[0] = if background { 11 } else { 3 };
        data[12] = 255;
        strout(&mut data, "art").unwrap();
        data
    }
    fn cel(layer: u16, x: i16, width: u16, pixels: &[u8], compressed: bool) -> Vec<u8> {
        let mut data = vec![0; 16];
        data[0..2].copy_from_slice(&layer.to_le_bytes());
        data[2..4].copy_from_slice(&x.to_le_bytes());
        data[6] = 255;
        if compressed {
            return encode_cel(&data, width, 1, pixels).unwrap();
        }
        data.extend(width.to_le_bytes());
        data.extend(1u16.to_le_bytes());
        data.extend(pixels);
        data
    }
    fn link(layer: u16, x: i16, frame: u16) -> Vec<u8> {
        let mut data = vec![0; 16];
        data[..2].copy_from_slice(&layer.to_le_bytes());
        data[2..4].copy_from_slice(&x.to_le_bytes());
        data[6] = 255;
        data[7] = 1;
        data.extend(frame.to_le_bytes());
        data
    }
    fn palette(colors: &[[u8; 4]]) -> Vec<u8> {
        write_palette(
            &colors
                .iter()
                .map(|&rgba| Some(Color { rgba, name: None }))
                .collect(),
        )
        .unwrap()
    }
    fn image(doc: &Document, frame: u32) -> Vec<u8> {
        // asefile 0.3.8 predates sprite user-data and rejects unknown chunks.
        // Exclude only these metadata chunks in this independent render oracle;
        // preservation and associations are separately checked on the real file.
        let mut render_doc = doc.clone();
        for frame in &mut render_doc.frames {
            frame
                .chunks
                .retain(|c| c.kind != 0xdead && c.kind != 0x2020);
        }
        asefile::AsepriteFile::read(std::io::Cursor::new(render_doc.encode().unwrap()))
            .unwrap()
            .frame(frame)
            .image()
            .into_raw()
    }
    fn metadata(text: &str) -> Vec<u8> {
        let mut data = 1u32.to_le_bytes().to_vec();
        strout(&mut data, text).unwrap();
        data
    }

    #[test]
    fn mirror_all_frames_layers_and_link_positions_with_independent_renderer() {
        let mut doc = fixture(
            32,
            vec![
                vec![
                    (0x2004, layer(false)),
                    (0x2020, metadata("layer")),
                    (0x2004, layer(false)),
                    (
                        0x2005,
                        cel(0, 1, 2, &[255, 0, 0, 255, 0, 255, 0, 255], false),
                    ),
                    (0x2020, metadata("cel")),
                    (0x2005, cel(1, 5, 1, &[0, 0, 255, 255], true)),
                    (0xdead, vec![1, 2, 3, 4]),
                ],
                vec![(0x2005, link(0, 4, 0)), (0x2005, link(1, 0, 0))],
            ],
        );
        let before: Vec<_> = (0..2).map(|f| image(&doc, f)).collect();
        let targets: Vec<_> = doc.targets().iter().map(|t| t.text.clone()).collect();
        doc.transform(ArtworkOperation::FlipHorizontal).unwrap();
        for f in 0..2 {
            let after = image(&doc, f);
            for y in 0..3 {
                for x in 0..8 {
                    assert_eq!(
                        &after[(y * 8 + x) * 4..(y * 8 + x + 1) * 4],
                        &before[f as usize][(y * 8 + 7 - x) * 4..(y * 8 + 8 - x) * 4]
                    );
                }
            }
        }
        assert_eq!(u16at(&doc.cel_at(1, 0).unwrap().data, 7).unwrap(), 1);
        assert_eq!(
            doc.targets()
                .iter()
                .map(|t| t.text.clone())
                .collect::<Vec<_>>(),
            targets
        );
        assert_eq!(doc.frames[0].chunks.last().unwrap().data, [1, 2, 3, 4]);
        doc.transform(ArtworkOperation::FlipHorizontal).unwrap();
        assert_eq!(image(&doc, 0), before[0]);
        assert_eq!(image(&doc, 1), before[1]);
    }

    #[test]
    fn expand_only_changes_dimensions_not_payloads_or_origin() {
        let mut doc = fixture(
            32,
            vec![vec![
                (0x2004, layer(false)),
                (0x2005, cel(0, -1, 2, &[1, 2, 3, 255, 4, 5, 6, 255], true)),
            ]],
        );
        let chunks = doc.frames[0]
            .chunks
            .iter()
            .map(|c| c.data.clone())
            .collect::<Vec<_>>();
        doc.transform(ArtworkOperation::ExpandCanvas {
            right: 7,
            bottom: 2,
        })
        .unwrap();
        assert_eq!(doc.dimensions(), (15, 5));
        assert_eq!(
            doc.frames[0]
                .chunks
                .iter()
                .map(|c| c.data.clone())
                .collect::<Vec<_>>(),
            chunks
        );
        let before = doc.encode().unwrap();
        assert!(doc
            .transform(ArtworkOperation::ExpandCanvas {
                right: u16::MAX,
                bottom: 0
            })
            .is_err());
        assert_eq!(doc.encode().unwrap(), before);
    }

    #[test]
    fn indexed_adds_exact_colors_preserves_metadata_and_roundtrips() {
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let transparent = [3, 4, 5, 0];
        let mut doc = fixture(
            32,
            vec![vec![
                (0x2019, palette(&[red])),
                (0x2020, metadata("sprite")),
                (0x2004, layer(false)),
                (0x2020, metadata("layer")),
                (
                    0x2005,
                    cel(0, 0, 3, &[red, green, transparent].concat(), true),
                ),
                (0x2020, metadata("cel")),
            ]],
        );
        let original = image(&doc, 0);
        let pixels = decode_cel(&doc.cel_at(0, 0).unwrap().data, 4, &mut { MAX_FILE })
            .unwrap()
            .2;
        doc.transform(ArtworkOperation::ConvertToIndexed).unwrap();
        assert_eq!(u16at(&doc.header, 12).unwrap(), 8);
        assert_eq!(image(&doc, 0), original);
        let colors = doc.palettes().unwrap();
        assert_eq!(colors[0][0].as_ref().unwrap().rgba, red);
        assert!(colors[0].iter().any(|c| c.as_ref().unwrap().rgba == green));
        assert_eq!(doc.targets()[0].text, "sprite");
        assert_eq!(doc.targets()[1].text, "layer");
        assert_eq!(doc.targets()[2].text, "cel");
        doc.transform(ArtworkOperation::ConvertToRgba).unwrap();
        assert_eq!(
            decode_cel(&doc.cel_at(0, 0).unwrap().data, 4, &mut { MAX_FILE })
                .unwrap()
                .2,
            pixels
        );
        assert_eq!(image(&doc, 0), original);
    }

    #[test]
    fn indexed_linked_cels_use_current_frame_palette_when_materialized() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let mut doc = fixture(
            8,
            vec![
                vec![
                    (0x2019, palette(&[[0; 4], red])),
                    (0x2004, layer(false)),
                    (0x2005, cel(0, 1, 1, &[1], true)),
                ],
                vec![(0x2019, palette(&[[0; 4], blue])), (0x2005, link(0, 3, 0))],
            ],
        );
        // asefile 0.3.8 incorrectly applies the final palette to all frames and
        // renders linked cels at source positions. Assert frame-specific colors
        // and link-specific coordinates directly, then render materialized RGBA.
        doc.transform(ArtworkOperation::ConvertToRgba).unwrap();
        assert_eq!(&image(&doc, 0)[4..8], &red);
        assert_eq!(&image(&doc, 1)[12..16], &blue);
        assert_eq!(
            decode_cel(&doc.cel_at(1, 0).unwrap().data, 4, &mut { MAX_FILE })
                .unwrap()
                .2,
            blue
        );
    }

    #[test]
    fn unsafe_conversion_is_atomic_for_multilayer_alpha_palette_overflow_and_bad_links() {
        for pixels in [
            vec![1, 2, 3, 128],
            vec![1, 2, 3, 0, 4, 5, 6, 0],
            (0..257)
                .flat_map(|i| [i as u8, (i >> 8) as u8, 0, 255])
                .collect(),
        ] {
            let mut doc = fixture(
                32,
                vec![vec![
                    (0x2004, layer(false)),
                    (0x2005, cel(0, 0, (pixels.len() / 4) as u16, &pixels, true)),
                ]],
            );
            // Multiple image layers use indexed compositing, which cannot preserve
            // partial alpha or different hidden RGB values.
            doc.frames[0].chunks.insert(
                1,
                Chunk {
                    uid: doc.next_uid,
                    kind: 0x2004,
                    data: layer(false),
                },
            );
            let before = doc.encode().unwrap();
            assert!(doc.transform(ArtworkOperation::ConvertToIndexed).is_err());
            assert_eq!(doc.encode().unwrap(), before);
        }
        let mut doc = fixture(
            32,
            vec![vec![(0x2004, layer(false)), (0x2005, link(0, 0, 0))]],
        );
        let before = doc.encode().unwrap();
        assert!(doc
            .transform(ArtworkOperation::FlipHorizontal)
            .unwrap_err()
            .contains("Cyclic"));
        assert_eq!(doc.encode().unwrap(), before);
    }

    #[test]
    fn single_layer_alpha_and_hidden_rgb_roundtrip_exactly() {
        let pixels = vec![1, 2, 3, 128, 10, 20, 30, 0, 40, 50, 60, 0];
        let mut doc = fixture(
            32,
            vec![vec![
                (0x2004, layer(false)),
                (0x2005, cel(0, 0, 3, &pixels, true)),
            ]],
        );
        let rendered = image(&doc, 0);
        doc.transform(ArtworkOperation::ConvertToIndexed).unwrap();
        assert_eq!(image(&doc, 0), rendered);
        doc.transform(ArtworkOperation::ConvertToRgba).unwrap();
        assert_eq!(
            decode_cel(&doc.cel_at(0, 0).unwrap().data, 4, &mut { MAX_FILE })
                .unwrap()
                .2,
            pixels
        );
    }

    #[test]
    fn indexed_multilayer_palette_alpha_is_rejected_except_transparent_slot() {
        for alpha in [0, 128] {
            let mut doc = fixture(
                8,
                vec![vec![
                    (
                        0x2019,
                        palette(&[[0, 0, 0, 0], [255, 0, 0, 255], [0, 0, 255, alpha]]),
                    ),
                    (0x2004, layer(false)),
                    (0x2004, layer(false)),
                    (0x2005, cel(0, 0, 1, &[1], false)),
                    (0x2005, cel(1, 0, 1, &[2], false)),
                ]],
            );
            let before = doc.encode().unwrap();
            assert!(doc
                .transform(ArtworkOperation::ConvertToRgba)
                .unwrap_err()
                .contains("Palette alpha"));
            assert_eq!(doc.encode().unwrap(), before);
        }
        let mut doc = fixture(
            8,
            vec![vec![
                (0x2019, palette(&[[0, 0, 0, 0], [255, 0, 0, 255]])),
                (0x2004, layer(false)),
                (0x2004, layer(false)),
                (0x2005, cel(0, 0, 1, &[1], false)),
                (0x2005, cel(1, 0, 1, &[0], false)),
            ]],
        );
        let before = image(&doc, 0);
        doc.transform(ArtworkOperation::ConvertToRgba).unwrap();
        assert_eq!(image(&doc, 0), before);
    }

    #[test]
    fn full_palette_reuses_unused_transparency_slot_without_reordering() {
        let colors: Vec<_> = (0..256).map(|i| [i as u8, 0, 0, 255]).collect();
        let mut doc = fixture(
            32,
            vec![vec![
                (0x2019, palette(&colors)),
                (0x2004, layer(false)),
                (0x2005, cel(0, 0, 1, &colors[0], false)),
            ]],
        );
        doc.transform(ArtworkOperation::ConvertToIndexed).unwrap();
        assert_ne!(doc.header[28], 0);
        assert_eq!(
            doc.palettes().unwrap()[0]
                .iter()
                .map(|c| c.as_ref().unwrap().rgba)
                .collect::<Vec<_>>(),
            colors
        );
        assert_eq!(&image(&doc, 0)[..4], &colors[0]);
    }

    #[test]
    fn grayscale_and_legacy_palette_decode_losslessly() {
        let mut gray = fixture(
            16,
            vec![vec![
                (0x2004, layer(false)),
                (0x2005, cel(0, 0, 2, &[20, 255, 90, 255], true)),
            ]],
        );
        let before = image(&gray, 0);
        gray.transform(ArtworkOperation::ConvertToIndexed).unwrap();
        assert_eq!(image(&gray, 0), before);
        gray.transform(ArtworkOperation::ConvertToRgba).unwrap();
        assert_eq!(image(&gray, 0), before);
        let mut legacy = fixture(
            8,
            vec![vec![
                (0x0004, vec![1, 0, 0, 2, 0, 0, 0, 5, 7, 9]),
                (0x2004, layer(false)),
                (0x2005, cel(0, 0, 1, &[1], false)),
            ]],
        );
        legacy.transform(ArtworkOperation::ConvertToRgba).unwrap();
        assert_eq!(&image(&legacy, 0)[..4], &[5, 7, 9, 255]);
    }
}
