//! Append animations without conflating their layers or tag identities.
use super::*;
use std::collections::BTreeSet;

impl Document {
    /// Return a new document and a description of its color-mode decision.
    /// Inputs are never modified. Unsupported global metadata fails explicitly.
    pub fn merge(&self, other: &Document) -> Result<(Document, String)> {
        let total = self
            .frames
            .len()
            .checked_add(other.frames.len())
            .ok_or("Too many frames")?;
        let total = u16::try_from(total).map_err(|_| "Merged animation exceeds 65535 frames")?;
        for doc in [self, other] {
            let layers = doc.image_layers()?;
            let mut budget = MAX_FILE;
            for (fi, frame) in doc.frames.iter().enumerate() {
                if !frame.tail.is_empty() {
                    return Err("Merge does not support opaque frame trailing data".into());
                }
                for chunk in &frame.chunks {
                    if chunk.kind == 0x2005 {
                        let layer = u16at(&chunk.data, 0)?;
                        if layer as usize >= layers.len() {
                            return Err("Cel references an invalid layer".into());
                        }
                        let source = doc.source_cel(fi, layer)?;
                        transforms::decode_cel(
                            &source.data,
                            u16at(&doc.header, 12)? as usize / 8,
                            &mut budget,
                        )?;
                    }
                    if !matches!(
                        chunk.kind,
                        4 | 0x11 | 0x2004 | 0x2005 | 0x2006 | 0x2007 | 0x2018 | 0x2019 | 0x2020
                    ) {
                        return Err(format!("Merge cannot safely relocate chunk 0x{:04x} (slices, tilesets, external references and unknown chunks are not supported)", chunk.kind));
                    }
                    if chunk.kind == 0x2004 && fi != 0 {
                        return Err(
                            "Merge requires all layer declarations in the first frame".into()
                        );
                    }
                }
            }
        }
        if self.header[14..18] != other.header[14..18] {
            return Err("Merge requires matching layer/group opacity and UUID header flags".into());
        }
        if self.header[34..44] != other.header[34..44] {
            return Err("Merge requires matching pixel aspect ratio and grid settings".into());
        }
        let profiles = |d: &Document| {
            d.frames
                .iter()
                .flat_map(|f| &f.chunks)
                .filter(|c| c.kind == 0x2007)
                .map(|c| c.data.clone())
                .collect::<Vec<_>>()
        };
        if profiles(self) != profiles(other) {
            return Err("Merge requires matching color profiles; convert both sprites to the same profile in Aseprite first".into());
        }
        let sprite_user = |d: &Document| d.locations[0].data.map(|id| d.chunk(id).clone());
        let a_user = sprite_user(self);
        let b_user = sprite_user(other);
        if matches!((&a_user, &b_user), (Some(a), Some(b)) if a.data != b.data) {
            return Err("Both sprites have different sprite-level user data. Reconcile it before merging; layer, cel and tag user data are preserved".into());
        }
        let was_indexed = u16at(&self.header, 12)? == 8 && u16at(&other.header, 12)? == 8;
        let palette = union_palette(self, other)?;
        let palette_count = u32at(&palette, 0)?;
        let mut a = self.clone();
        let mut b = other.clone();
        a.transform(ArtworkOperation::ConvertToRgba)?;
        b.transform(ArtworkOperation::ConvertToRgba)?;
        let layer_count = a
            .frames
            .iter()
            .flat_map(|f| &f.chunks)
            .filter(|c| c.kind == 0x2004)
            .count();
        let b_layers = b
            .frames
            .iter()
            .flat_map(|f| &f.chunks)
            .filter(|c| c.kind == 0x2004)
            .count();
        if layer_count + b_layers > 65536 {
            return Err("Merged sprite exceeds Aseprite's layer index limit".into());
        }
        let offset = a.frames.len() as u16;
        let mut tags = Vec::new();
        for (doc, frame_offset) in [(&a, 0u16), (&b, offset)] {
            for tag in &doc.tags {
                if tag.from > tag.to || tag.to as usize >= doc.frames.len() {
                    return Err("Cannot merge an out-of-range animation tag".into());
                }
                let (fi, uid, ordinal) = doc.tag_locations[tag.id];
                let chunk = doc.frames[fi].chunks.iter().find(|c| c.uid == uid).unwrap();
                let entries = tag_entries(&chunk.data)?;
                let mut entry = entries[ordinal].clone();
                entry[..2].copy_from_slice(&(tag.from + frame_offset).to_le_bytes());
                entry[2..4].copy_from_slice(&(tag.to + frame_offset).to_le_bytes());
                let data = doc
                    .locations
                    .iter()
                    .find(|l| l.owner == Some(uid) && l.ordinal == ordinal)
                    .and_then(|l| l.data)
                    .map(|id| doc.chunk(id).data.clone())
                    .unwrap_or_else(|| vec![0; 4]);
                tags.push((entry, data));
            }
        }
        let count = u16::try_from(tags.len()).map_err(|_| "Merged sprite exceeds 65535 tags")?;
        let mut layers = Vec::new();
        for (doc, second) in [(&mut a, false), (&mut b, true)] {
            let mut remove = BTreeSet::new();
            let mut layer_users = std::collections::BTreeMap::new();
            for loc in &doc.locations {
                if let (Some(owner), Some(data)) = (loc.owner, loc.data) {
                    if doc.chunk(owner).kind == 0x2004 {
                        layer_users.insert(owner, doc.chunk(data).clone());
                        remove.insert(data);
                    }
                }
                if loc.owner.is_none() || doc.chunk(loc.owner.unwrap()).kind == 0x2018 {
                    if let Some(id) = loc.data {
                        remove.insert(id);
                    }
                }
            }
            for frame in &mut doc.frames {
                // Zero frame durations inherit each source's speed, not the merged header.
                if u16at(&frame.header, 8)? == 0 {
                    frame.header[8..10].copy_from_slice(&doc.header[18..20]);
                }
                let mut kept = Vec::new();
                let chunks = std::mem::take(&mut frame.chunks);
                for mut c in chunks {
                    if remove.contains(&c.uid)
                        || matches!(c.kind, 4 | 0x11 | 0x2019 | 0x2018)
                        || (second && c.kind == 0x2007)
                    {
                        continue;
                    }
                    if c.kind == 0x2004 {
                        let user = layer_users.remove(&c.uid);
                        layers.push(c);
                        if let Some(user) = user {
                            layers.push(user);
                        }
                        continue;
                    }
                    if c.kind == 0x2005 && second {
                        let index = u16at(&c.data, 0)? as usize;
                        if index >= b_layers {
                            return Err("Cel references an invalid source layer".into());
                        }
                        c.data[..2].copy_from_slice(&((index + layer_count) as u16).to_le_bytes());
                        if u16at(&c.data, 7)? == 1 {
                            let link = u16at(&c.data, 16)?;
                            if link as usize >= other.frames.len() {
                                return Err("Linked cel references an invalid frame".into());
                            }
                            c.data[16..18].copy_from_slice(&(link + offset).to_le_bytes());
                        }
                    }
                    kept.push(c);
                }
                frame.chunks = kept;
            }
        }
        let mut prefix = vec![Chunk {
            uid: 0,
            kind: 0x2019,
            data: palette,
        }];
        if let Some(c) = a_user.or(b_user) {
            prefix.push(c);
        }
        prefix.extend(layers);
        if count != 0 {
            let mut data = count.to_le_bytes().to_vec();
            data.extend([0; 8]);
            for (entry, _) in &tags {
                data.extend(entry);
            }
            prefix.push(Chunk {
                uid: 0,
                kind: 0x2018,
                data,
            });
            for (_, data) in tags {
                prefix.push(Chunk {
                    uid: 0,
                    kind: 0x2020,
                    data,
                });
            }
        }
        prefix.append(&mut a.frames[0].chunks);
        a.frames[0].chunks = prefix;
        a.frames.extend(b.frames);
        a.header[6..8].copy_from_slice(&total.to_le_bytes());
        a.header[32..34].copy_from_slice(
            &(if palette_count == 256 {
                0
            } else {
                palette_count.min(65535) as u16
            })
            .to_le_bytes(),
        );
        for p in [8, 10] {
            a.header[p..p + 2].copy_from_slice(
                &u16at(&self.header, p)?
                    .max(u16at(&other.header, p)?)
                    .to_le_bytes(),
            );
        }
        // Reparse assigns unique chunk IDs and reconstructs all metadata associations.
        let mut merged = Document::parse(&a.encode()?)?;
        let note = if was_indexed {
            match merged.transform(ArtworkOperation::ConvertToIndexed) {
                Ok(()) => "Merged as indexed with an exact palette union".to_string(),
                Err(reason) => format!("Merged as lossless RGBA instead of indexed: {reason}"),
            }
        } else {
            "Merged as RGBA with an exact palette union".to_string()
        };
        Ok((merged, note))
    }
}

fn tag_entries(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut r = Reader::new(data);
    let count = r.u16()?;
    r.take(8)?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let start = r.pos;
        r.take(17)?;
        r.string()?;
        entries.push(data[start..r.pos].to_vec());
    }
    if r.pos != data.len() {
        return Err("Cannot safely merge extended tag data".into());
    }
    Ok(entries)
}

// Keep unused swatches too. RGBA palettes can contain more than 256 entries.
fn union_palette(a: &Document, b: &Document) -> Result<Vec<u8>> {
    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut seen = BTreeSet::new();
    for doc in [a, b] {
        for frame in &doc.frames {
            let modern = frame.chunks.iter().any(|c| c.kind == 0x2019);
            for c in &frame.chunks {
                if c.kind == 0x2019 {
                    let mut r = Reader::new(&c.data);
                    let size = r.u32()?;
                    let first = r.u32()?;
                    let last = r.u32()?;
                    r.take(8)?;
                    if last < first || last >= size || size > 65536 {
                        return Err("Invalid palette range".into());
                    }
                    for _ in first..=last {
                        let start = r.pos;
                        let flags = r.u16()?;
                        if flags & !1 != 0 {
                            return Err("Unknown palette entry flags".into());
                        }
                        r.take(4)?;
                        if flags & 1 != 0 {
                            r.string()?;
                        }
                        let entry = c.data[start..r.pos].to_vec();
                        if seen.insert(entry.clone()) {
                            entries.push(entry);
                        }
                    }
                    if r.pos != c.data.len() {
                        return Err("Unexpected palette trailing data".into());
                    }
                } else if matches!(c.kind, 4 | 0x11) && !modern {
                    let mut r = Reader::new(&c.data);
                    let packets = r.u16()?;
                    for _ in 0..packets {
                        r.u8()?;
                        let n = r.u8()?;
                        for _ in 0..if n == 0 { 256 } else { n as usize } {
                            let mut rgb = [r.u8()?, r.u8()?, r.u8()?];
                            if c.kind == 0x11 {
                                for v in &mut rgb {
                                    if *v > 63 {
                                        return Err("Invalid six-bit palette color".into());
                                    }
                                    *v = (*v << 2) | (*v >> 4);
                                }
                            }
                            let e = vec![0, 0, rgb[0], rgb[1], rgb[2], 255];
                            if seen.insert(e.clone()) {
                                entries.push(e);
                            }
                        }
                    }
                }
                if entries.len() > 65536 {
                    return Err("Merged palette exceeds safety limit".into());
                }
            }
        }
    }
    if entries.is_empty() {
        entries.push(vec![0; 6]);
    }
    let mut out = (entries.len() as u32).to_le_bytes().to_vec();
    out.extend(0u32.to_le_bytes());
    out.extend(((entries.len() - 1) as u32).to_le_bytes());
    out.extend([0; 8]);
    for entry in entries {
        out.extend(entry);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(width: u16, height: u16, rgba: [u8; 4], linked: bool) -> Document {
        let mut header = vec![0; 128];
        header[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
        header[6..8].copy_from_slice(&(if linked { 2u16 } else { 1 }).to_le_bytes());
        header[8..10].copy_from_slice(&width.to_le_bytes());
        header[10..12].copy_from_slice(&height.to_le_bytes());
        header[12..14].copy_from_slice(&32u16.to_le_bytes());
        header[14] = 1;
        let mut layer = vec![0; 16];
        layer[0] = 1;
        layer[12] = 255;
        strout(&mut layer, "same-name").unwrap();
        let mut cel = vec![0; 20];
        cel[6] = 255;
        cel[16] = 1;
        cel[18] = 1;
        cel.extend(rgba);
        let mut tags = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        tags.extend([0; 17]);
        tags[14] = 2;
        tags[15] = 3;
        strout(&mut tags, "idle").unwrap();
        let mut user = 1u32.to_le_bytes().to_vec();
        strout(&mut user, "tag annotation").unwrap();
        let mut frame_header = vec![0; 16];
        frame_header[4..6].copy_from_slice(&0xf1fau16.to_le_bytes());
        frame_header[8] = 75;
        let chunks = vec![
            (0x2004, layer),
            (0x2005, cel),
            (0x2018, tags),
            (0x2020, user),
        ]
        .into_iter()
        .enumerate()
        .map(|(uid, (kind, data))| Chunk { uid, kind, data })
        .collect();
        let mut frames = vec![Frame {
            header: frame_header.clone(),
            chunks,
            tail: vec![],
        }];
        if linked {
            let mut data = vec![0; 18];
            data[6] = 255;
            data[7] = 1;
            data[2] = 1;
            frame_header[8] = 125;
            frames.push(Frame {
                header: frame_header,
                chunks: vec![Chunk {
                    uid: 4,
                    kind: 0x2005,
                    data,
                }],
                tail: vec![],
            });
        }
        let doc = Document {
            header,
            frames,
            targets: vec![],
            locations: vec![],
            tags: vec![],
            tag_locations: vec![],
            next_uid: 5,
        };
        Document::parse(&doc.encode().unwrap()).unwrap()
    }
    #[test]
    fn merge_roundtrip_preserves_pixels_tags_layers_links_durations_and_inputs() {
        let a = fixture(2, 1, [255, 0, 0, 255], false);
        let b = fixture(1, 2, [0, 0, 255, 255], true);
        let before_a = a.encode().unwrap();
        let before_b = b.encode().unwrap();
        let (merged, _) = a.merge(&b).unwrap();
        let bytes = merged.encode().unwrap();
        let decoded = asefile::AsepriteFile::read(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height(), decoded.num_frames()),
            (2, 2, 3)
        );
        assert_eq!(decoded.frame(0).image().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(decoded.frame(1).image().get_pixel(0, 0).0, [0, 0, 255, 255]);
        // asefile 0.3.8 renders links at the source cel position; validate the
        // linked cel placement and remapped reference directly below instead.
        let reread = Document::parse(&bytes).unwrap();
        assert_eq!(reread.tags.len(), 2);
        assert_eq!(
            (reread.tags[0].name.as_str(), reread.tags[1].name.as_str()),
            ("idle", "idle")
        );
        assert_eq!(
            (
                reread.tags[1].from,
                reread.tags[1].to,
                reread.tags[1].direction,
                reread.tags[1].repeat
            ),
            (1, 1, 2, 3)
        );
        assert_eq!(
            reread
                .targets
                .iter()
                .filter(|t| t.kind == Scope::Tag && t.text == "tag annotation")
                .count(),
            2
        );
        assert_eq!(
            reread
                .targets
                .iter()
                .filter(|t| t.kind == Scope::Layer)
                .count(),
            2
        );
        assert_eq!(u16at(&reread.frames[2].header, 8).unwrap(), 125);
        let link = &reread.frames[2].chunks[0];
        assert_eq!(u16at(&link.data, 0).unwrap(), 1);
        assert_eq!(u16at(&link.data, 2).unwrap(), 1);
        assert_eq!(u16at(&link.data, 16).unwrap(), 1);
        assert_eq!(a.encode().unwrap(), before_a);
        assert_eq!(b.encode().unwrap(), before_b);
    }
    #[test]
    fn merge_indexed_palettes_remaps_conflicting_indices_and_retains_unused_swatches() {
        let mut a = fixture(1, 1, [255, 0, 0, 255], false);
        let mut b = fixture(1, 1, [0, 0, 255, 255], false);
        for (doc, unused) in [(&mut a, [23, 42, 11, 255]), (&mut b, [41, 12, 22, 255])] {
            let colors = vec![Some(transforms::Color {
                rgba: unused,
                name: Some("unused".into()),
            })];
            doc.frames[0].chunks.insert(
                0,
                Chunk {
                    uid: 999,
                    kind: 0x2019,
                    data: transforms::write_palette(&colors).unwrap(),
                },
            );
            *doc = Document::parse(&doc.encode().unwrap()).unwrap();
            doc.transform(ArtworkOperation::ConvertToIndexed).unwrap();
        }
        let (merged, note) = a.merge(&b).unwrap();
        assert!(note.starts_with("Merged as indexed"));
        assert_eq!(u16at(&merged.header, 12).unwrap(), 8);
        let decoded =
            asefile::AsepriteFile::read(std::io::Cursor::new(merged.encode().unwrap())).unwrap();
        assert_eq!(decoded.frame(0).image().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(decoded.frame(1).image().get_pixel(0, 0).0, [0, 0, 255, 255]);
        let palettes = merged.palettes().unwrap();
        for unused in [[23, 42, 11, 255], [41, 12, 22, 255]] {
            assert!(palettes[0]
                .iter()
                .flatten()
                .any(|c| c.rgba == unused && c.name.as_deref() == Some("unused")));
        }
    }
    #[test]
    fn palette_union_overflow_falls_back_without_losing_colors() {
        let mut a = fixture(1, 1, [255, 0, 0, 255], false);
        let mut b = fixture(1, 1, [0, 0, 255, 255], false);
        for (doc, green) in [(&mut a, 0), (&mut b, 1)] {
            let mut colors = vec![Some(transforms::Color {
                rgba: [0, 0, 0, 0],
                name: None,
            })];
            colors.extend((0..200).map(|r| {
                Some(transforms::Color {
                    rgba: [r, green, 0, 255],
                    name: None,
                })
            }));
            doc.frames[0].chunks.insert(
                0,
                Chunk {
                    uid: 999,
                    kind: 0x2019,
                    data: transforms::write_palette(&colors).unwrap(),
                },
            );
            *doc = Document::parse(&doc.encode().unwrap()).unwrap();
            doc.transform(ArtworkOperation::ConvertToIndexed).unwrap();
        }
        let (merged, note) = a.merge(&b).unwrap();
        assert_eq!(u16at(&merged.header, 12).unwrap(), 32);
        assert!(note.contains("lossless RGBA"));
        assert!(u32at(&merged.frames[0].chunks[0].data, 0).unwrap() > 256);
        let decoded =
            asefile::AsepriteFile::read(std::io::Cursor::new(merged.encode().unwrap())).unwrap();
        assert_eq!(decoded.frame(0).image().get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(decoded.frame(1).image().get_pixel(0, 0).0, [0, 0, 255, 255]);
    }
    #[test]
    fn invalid_rgba_links_are_rejected_before_merge() {
        let a = fixture(1, 1, [1, 2, 3, 255], false);
        let mut b = fixture(1, 1, [4, 5, 6, 255], true);
        b.frames[1].chunks[0].data[16..18].copy_from_slice(&1u16.to_le_bytes());
        assert!(a.merge(&b).err().unwrap().contains("Cyclic"));
    }
    #[test]
    fn differing_sprite_metadata_is_not_silently_discarded() {
        let mut a = fixture(1, 1, [1, 2, 3, 255], false);
        let mut b = fixture(1, 1, [4, 5, 6, 255], false);
        for (doc, text) in [(&mut a, "first"), (&mut b, "second")] {
            let mut data = 1u32.to_le_bytes().to_vec();
            strout(&mut data, text).unwrap();
            doc.frames[0].chunks.insert(
                0,
                Chunk {
                    uid: 998,
                    kind: 0x2020,
                    data,
                },
            );
            *doc = Document::parse(&doc.encode().unwrap()).unwrap();
        }
        assert!(a
            .merge(&b)
            .err()
            .unwrap()
            .contains("sprite-level user data"));
    }
    #[test]
    fn incompatible_metadata_fails_without_mutating_inputs() {
        let a = fixture(1, 1, [1, 2, 3, 255], false);
        let mut b = a.clone();
        b.frames[0].chunks.push(Chunk {
            uid: 999,
            kind: 0x7777,
            data: vec![1, 2],
        });
        let before = b.encode().unwrap();
        assert!(a.merge(&b).err().unwrap().contains("0x7777"));
        assert_eq!(b.encode().unwrap(), before);
    }
}
