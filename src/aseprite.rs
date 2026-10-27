//! Lossless container editor. No pixel decompression or image re-encoding occurs.
//! Layout follows aseprite/docs/ase-file-specs.md; association rules cross-checked
//! against Aseprite's MIT dio decoder and LibreSprite's reader.
//! https://github.com/aseprite/aseprite/blob/main/docs/ase-file-specs.md
//! https://github.com/aseprite/aseprite/blob/main/src/dio/aseprite_decoder.cpp
//! https://github.com/LibreSprite/LibreSprite/blob/master/src/app/file/ase_format.cpp
//! The tests also decode edited files with the independent MIT asefile crate.
use aseprite_loader::binary::chunks::{
    layer::parse_layer_chunk,
    tags::{parse_tags_chunk, AnimationDirection},
    user_data::parse_user_data_chunk,
};
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, String>;
const MAX_FILE: usize = 256 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Sprite,
    Layer,
    Cel,
    Tag,
    Slice,
    Tileset,
    Tile,
}
#[derive(Clone, Debug)]
pub struct Target {
    pub id: usize,
    pub label: String,
    pub kind: Scope,
    pub text: String,
    pub color: Option<[u8; 4]>,
    pub properties: Option<Value>,
    pub warning: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Tag {
    pub id: usize,
    pub name: String,
    pub from: u16,
    pub to: u16,
    pub direction: u8,
    pub repeat: u16,
    pub color: [u8; 3],
}
#[derive(Clone)]
struct Chunk {
    uid: usize,
    kind: u16,
    data: Vec<u8>,
}
#[derive(Clone)]
struct Frame {
    header: Vec<u8>,
    chunks: Vec<Chunk>,
    tail: Vec<u8>,
}
#[derive(Clone)]
struct Location {
    frame: usize,
    owner: Option<usize>,
    data: Option<usize>,
    ordinal: usize,
}
#[derive(Clone)]
pub struct Document {
    header: Vec<u8>,
    frames: Vec<Frame>,
    targets: Vec<Target>,
    locations: Vec<Location>,
    tags: Vec<Tag>,
    tag_locations: Vec<(usize, usize, usize)>,
    next_uid: usize,
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or("Length overflow")?;
        let b = self.bytes.get(self.pos..end).ok_or("Truncated metadata")?;
        self.pos = end;
        Ok(b)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "Invalid UTF-8 metadata".into())
    }
    fn count(&mut self) -> Result<usize> {
        let n = self.u32()? as usize;
        if n > MAX_ITEMS {
            Err("Too many metadata entries".into())
        } else {
            Ok(n)
        }
    }
}
fn strout(b: &mut Vec<u8>, s: &str) -> Result<()> {
    let n = u16::try_from(s.len()).map_err(|_| "Text exceeds Aseprite's 65535 byte limit")?;
    b.extend(n.to_le_bytes());
    b.extend(s.as_bytes());
    Ok(())
}
fn u16at(b: &[u8], p: usize) -> Result<u16> {
    Reader::new(b.get(p..).ok_or("Truncated field")?).u16()
}
fn u32at(b: &[u8], p: usize) -> Result<u32> {
    Reader::new(b.get(p..).ok_or("Truncated field")?).u32()
}
impl Document {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 128 || bytes.len() > MAX_FILE {
            return Err("File must be between 128 bytes and 256 MiB".into());
        }
        if u16at(bytes, 4)? != 0xa5e0 || u32at(bytes, 0)? as usize != bytes.len() {
            return Err("Invalid Aseprite header or file length".into());
        }
        let count = u16at(bytes, 6)? as usize;
        if count == 0
            || u16at(bytes, 8)? == 0
            || u16at(bytes, 10)? == 0
            || ![8, 16, 32].contains(&u16at(bytes, 12)?)
        {
            return Err("Invalid dimensions, frame count or color depth".into());
        }
        let mut r = Reader::new(&bytes[128..]);
        let mut frames = Vec::new();
        let mut uid = 0;
        for _ in 0..count {
            let start = r.pos;
            let len = r.u32()? as usize;
            if len < 16 {
                return Err("Invalid frame length".into());
            }
            r.pos = start;
            let frame = r.take(len)?;
            if u16at(frame, 4)? != 0xf1fa {
                return Err("Invalid frame magic".into());
            }
            let new = u32at(frame, 12)?;
            let n = if new == 0 {
                u16at(frame, 6)? as usize
            } else {
                new as usize
            };
            if n > MAX_ITEMS {
                return Err("Too many chunks".into());
            }
            let mut cr = Reader::new(&frame[16..]);
            let mut chunks = Vec::new();
            for _ in 0..n {
                let size = cr.u32()? as usize;
                let kind = cr.u16()?;
                if size < 6 {
                    return Err("Invalid chunk length".into());
                }
                if uid >= MAX_ITEMS {
                    return Err("Too many total chunks".into());
                }
                chunks.push(Chunk {
                    uid,
                    kind,
                    data: cr.take(size - 6)?.to_vec(),
                });
                uid += 1;
            }
            frames.push(Frame {
                header: frame[..16].to_vec(),
                chunks,
                tail: cr.bytes[cr.pos..].to_vec(),
            });
        }
        if r.pos != r.bytes.len() {
            return Err("Trailing bytes outside frames".into());
        }
        let mut d = Self {
            header: bytes[..128].to_vec(),
            frames,
            targets: vec![],
            locations: vec![],
            tags: vec![],
            tag_locations: vec![],
            next_uid: uid,
        };
        d.index()?;
        Ok(d)
    }
    fn add_target(
        &mut self,
        kind: Scope,
        label: String,
        frame: usize,
        owner: Option<usize>,
        ordinal: usize,
    ) -> usize {
        let id = self.targets.len();
        self.targets.push(Target {
            id,
            label,
            kind,
            text: String::new(),
            color: None,
            properties: None,
            warning: None,
        });
        self.locations.push(Location {
            frame,
            owner,
            data: None,
            ordinal,
        });
        id
    }
    fn index(&mut self) -> Result<()> {
        self.add_target(Scope::Sprite, "Sprite".into(), 0, None, 0);
        let mut current = Some(0);
        let mut queue = std::collections::VecDeque::new();
        let mut layer = 0;
        for fi in 0..self.frames.len() {
            for c in self.frames[fi].chunks.clone() {
                match c.kind {
                    0x2004 => {
                        // The library reads the name/type; we retain raw flags and
                        // all bytes (its layer flag model is not used for writing).
                        if c.data.len() < 18 {
                            return Err("Truncated layer".into());
                        }
                        let (_, parsed) = parse_layer_chunk(&c.data)
                            .map_err(|e| format!("Invalid layer: {e:?}"))?;
                        let name = parsed.name;
                        current = Some(self.add_target(
                            Scope::Layer,
                            format!("Layer {}: {}", layer + 1, name),
                            fi,
                            Some(c.uid),
                            0,
                        ));
                        layer += 1;
                        queue.clear();
                    }
                    0x2005 => {
                        let index = u16at(&c.data, 0)?;
                        if c.data.len() < 16 {
                            return Err("Truncated cel".into());
                        }
                        current = Some(self.add_target(
                            Scope::Cel,
                            format!("Frame {}, layer {}", fi + 1, index + 1),
                            fi,
                            Some(c.uid),
                            0,
                        ));
                        queue.clear();
                    }
                    0x2018 => {
                        let n = u16at(&c.data, 0)? as usize;
                        if self.targets.len() + n > MAX_ITEMS {
                            return Err("Too many metadata targets".into());
                        }
                        // Each tag needs 17 fixed bytes and a two-byte string length.
                        // Bound count before the library allocates its result vector.
                        if c.data.len() < 10 || n > (c.data.len() - 10) / 19 {
                            return Err("Truncated tags".into());
                        }
                        let (_, parsed) = parse_tags_chunk(&c.data)
                            .map_err(|e| format!("Invalid tags: {e:?}"))?;
                        queue.clear();
                        for (ordinal, parsed_tag) in parsed.tags.into_iter().enumerate() {
                            let from = *parsed_tag.frames.start();
                            let to = *parsed_tag.frames.end();
                            let direction = match parsed_tag.animation_direction {
                                AnimationDirection::Forward => 0,
                                AnimationDirection::Reverse => 1,
                                AnimationDirection::PingPong => 2,
                                AnimationDirection::PingPongReverse => 3,
                                AnimationDirection::Unknown(value) => value,
                            };
                            let repeat = parsed_tag.animation_repeat;
                            #[allow(deprecated)]
                            let color = parsed_tag.color;
                            let name = parsed_tag.name.to_owned();
                            let id = self.tags.len();
                            self.tags.push(Tag {
                                id,
                                name: name.clone(),
                                from,
                                to,
                                direction,
                                repeat,
                                color,
                            });
                            self.tag_locations.push((fi, c.uid, ordinal));
                            queue.push_back(self.add_target(
                                Scope::Tag,
                                format!("Tag: {name}"),
                                fi,
                                Some(c.uid),
                                ordinal,
                            ));
                        }
                        current = None;
                    }
                    0x2022 => {
                        let mut r = Reader::new(&c.data);
                        r.take(12)?;
                        let name = r.string()?;
                        current = Some(self.add_target(
                            Scope::Slice,
                            format!("Slice: {name}"),
                            fi,
                            Some(c.uid),
                            0,
                        ));
                        queue.clear();
                    }
                    0x2023 => {
                        let tiles = u32at(&c.data, 8)? as usize;
                        if self.targets.len() + tiles + 1 > MAX_ITEMS {
                            return Err("Too many tiles".into());
                        }
                        let mut r = Reader::new(&c.data);
                        r.take(32)?;
                        let name = r.string()?;
                        queue.clear();
                        let id = self.add_target(
                            Scope::Tileset,
                            format!("Tileset: {name}"),
                            fi,
                            Some(c.uid),
                            0,
                        );
                        queue.push_back(id);
                        for i in 0..tiles {
                            queue.push_back(self.add_target(
                                Scope::Tile,
                                format!("{name} / tile {i}"),
                                fi,
                                Some(c.uid),
                                i + 1,
                            ));
                        }
                        current = None;
                    }
                    0x2020 => {
                        let target = queue.pop_front().or(current);
                        if let Some(id) = target {
                            if self.locations[id].data.is_some() {
                                return Err("Ambiguous duplicate user data".into());
                            }
                            let (text, color, props, warning) = read_user(&c.data)?;
                            let t = &mut self.targets[id];
                            t.text = text;
                            t.color = color;
                            t.properties = props;
                            t.warning = warning;
                            self.locations[id].data = Some(c.uid);
                        } else {
                            return Err("User data has no supported owner".into());
                        }
                    }
                    _ => {} // Unknown chunks are retained, never interpreted as user data.
                }
            }
        }
        Ok(())
    }
    pub fn targets(&self) -> Vec<Target> {
        self.targets.clone()
    }
    pub fn tags(&self) -> Vec<Tag> {
        self.tags.clone()
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
    fn chunk(&self, uid: usize) -> &Chunk {
        self.frames
            .iter()
            .flat_map(|f| &f.chunks)
            .find(|c| c.uid == uid)
            .unwrap()
    }
    fn change_user(
        &mut self,
        id: usize,
        text: Option<&str>,
        color: Option<Option<[u8; 4]>>,
        properties: Option<&Value>,
    ) -> Result<()> {
        let loc = self
            .locations
            .get(id)
            .ok_or("Unknown metadata target")?
            .clone();
        let original = loc
            .data
            .map(|uid| self.chunk(uid).data.clone())
            .unwrap_or_else(|| vec![0; 4]);
        let flags = u32at(&original, 0)?;
        if flags & !7 != 0 {
            return Err("Future user-data flags: read-only".into());
        }
        let (old_text, old_color, prefix_len) = read_user_prefix(&original)?;
        let tail = &original[prefix_len..];
        let new_text = text.map(str::to_owned).or(old_text);
        let new_color = color.unwrap_or(old_color);
        let mut f = flags & 4;
        if new_text.is_some() {
            f |= 1
        }
        if new_color.is_some() {
            f |= 2
        }
        if properties.is_some() {
            f |= 4
        }
        let mut data = f.to_le_bytes().to_vec();
        if let Some(t) = &new_text {
            strout(&mut data, t)?
        }
        if let Some(c) = new_color {
            data.extend(c)
        }
        if let Some(p) = properties {
            data.extend(write_maps(p)?)
        } else {
            data.extend(tail)
        }
        let (t, c, p, w) = read_user(&data)?;
        if let Some(uid) = loc.data {
            self.frames[loc.frame]
                .chunks
                .iter_mut()
                .find(|c| c.uid == uid)
                .unwrap()
                .data = data;
        } else {
            // Preserve ordinal association by inserting empty placeholders for missing
            // earlier tag/tile user-data entries before appending this one.
            let mut related: Vec<usize> = self
                .locations
                .iter()
                .enumerate()
                .filter(|(_, l)| {
                    l.frame == loc.frame
                        && l.owner == loc.owner
                        && l.ordinal <= loc.ordinal
                        && l.data.is_none()
                })
                .map(|(i, _)| i)
                .collect();
            related.sort_by_key(|i| self.locations[*i].ordinal);
            for ti in related {
                let l = self.locations[ti].clone();
                let insertion = if let Some(owner) = l.owner {
                    let owner_pos = self.frames[l.frame]
                        .chunks
                        .iter()
                        .position(|c| c.uid == owner)
                        .unwrap();
                    owner_pos
                        + 1
                        + self
                            .locations
                            .iter()
                            .filter(|p| {
                                p.owner == l.owner && p.frame == l.frame && p.data.is_some()
                            })
                            .count()
                } else {
                    let leading = self.frames[0]
                        .chunks
                        .iter()
                        .take_while(|c| {
                            !matches!(c.kind, 0x2004 | 0x2005 | 0x2018 | 0x2022 | 0x2023)
                        })
                        .enumerate()
                        .filter_map(|(i, c)| matches!(c.kind, 4 | 0x11 | 0x2019).then_some(i))
                        .last();
                    if let Some(p) = leading {
                        if self.frames[0].chunks[p].kind != 0x2019 {
                            p + 1
                        } else {
                            let uid = self.next_uid;
                            self.next_uid += 1;
                            self.frames[0].chunks.insert(
                                p + 1,
                                Chunk {
                                    uid,
                                    kind: 4,
                                    data: vec![0, 0],
                                },
                            );
                            p + 2
                        }
                    } else {
                        let uid = self.next_uid;
                        self.next_uid += 1;
                        self.frames[0].chunks.insert(
                            0,
                            Chunk {
                                uid,
                                kind: 4,
                                data: vec![0, 0],
                            },
                        );
                        1
                    }
                };
                let uid = self.next_uid;
                self.next_uid += 1;
                self.frames[l.frame].chunks.insert(
                    insertion,
                    Chunk {
                        uid,
                        kind: 0x2020,
                        data: if ti == id { data.clone() } else { vec![0; 4] },
                    },
                );
                self.locations[ti].data = Some(uid);
            }
        }
        self.targets[id].text = t;
        self.targets[id].color = c;
        self.targets[id].properties = p;
        self.targets[id].warning = w;
        Ok(())
    }
    pub fn set_text(&mut self, id: usize, text: &str) -> Result<()> {
        self.change_user(id, Some(text), None, None)
    }
    pub fn set_color(&mut self, id: usize, color: Option<[u8; 4]>) -> Result<()> {
        self.change_user(id, None, Some(color), None)
    }
    pub fn set_properties(&mut self, id: usize, p: &Value) -> Result<()> {
        if self
            .targets
            .get(id)
            .ok_or("Unknown target")?
            .warning
            .is_some()
        {
            return Err("Unsupported properties are read-only".into());
        }
        self.change_user(id, None, None, Some(p))
    }
    pub fn set_tag(&mut self, tag: &Tag) -> Result<()> {
        if tag.from > tag.to || tag.to as usize >= self.frames.len() || tag.direction > 3 {
            return Err("Invalid tag frame range or direction".into());
        }
        let (fi, uid, ordinal) = *self.tag_locations.get(tag.id).ok_or("Unknown tag")?;
        let original = self.chunk(uid).data.clone();
        let mut r = Reader::new(&original);
        let n = r.u16()? as usize;
        r.take(8)?;
        let mut start = 0;
        let mut end = 0;
        for i in 0..n {
            let s = r.pos;
            r.take(17)?;
            r.string()?;
            if i == ordinal {
                start = s;
                end = r.pos;
            }
        }
        let mut replacement = original[start..start + 17].to_vec();
        replacement[0..2].copy_from_slice(&tag.from.to_le_bytes());
        replacement[2..4].copy_from_slice(&tag.to.to_le_bytes());
        replacement[4] = tag.direction;
        replacement[5..7].copy_from_slice(&tag.repeat.to_le_bytes());
        replacement[13..16].copy_from_slice(&tag.color);
        strout(&mut replacement, &tag.name)?;
        let mut b = original[..start].to_vec();
        b.extend(replacement);
        b.extend(&original[end..]);
        self.frames[fi]
            .chunks
            .iter_mut()
            .find(|c| c.uid == uid)
            .unwrap()
            .data = b;
        self.tags[tag.id] = tag.clone();
        for (target, location) in self.targets.iter_mut().zip(&self.locations) {
            if target.kind == Scope::Tag
                && location.owner == Some(uid)
                && location.frame == fi
                && location.ordinal == ordinal
            {
                target.label = format!("Tag: {}", tag.name);
            }
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut b = self.header.clone();
        for f in &self.frames {
            let start = b.len();
            let mut h = f.header.clone();
            let count = f.chunks.len() as u32;
            let old = u32at(&h, 12)?;
            let old = if old == 0 { u16at(&h, 6)? as u32 } else { old };
            if old != count {
                h[6..8].copy_from_slice(&(count.min(65535) as u16).to_le_bytes());
                h[12..16].copy_from_slice(&count.to_le_bytes());
            }
            b.extend(h);
            for c in &f.chunks {
                let size = u32::try_from(c.data.len() + 6).map_err(|_| "Chunk too large")?;
                b.extend(size.to_le_bytes());
                b.extend(c.kind.to_le_bytes());
                b.extend(&c.data);
            }
            b.extend(&f.tail);
            let size = u32::try_from(b.len() - start).map_err(|_| "Frame too large")?;
            b[start..start + 4].copy_from_slice(&size.to_le_bytes());
        }
        if b.len() > MAX_FILE {
            return Err("Result exceeds 256 MiB".into());
        }
        let len = b.len() as u32;
        b[..4].copy_from_slice(&len.to_le_bytes());
        Ok(b)
    }
}
type UserFields = (String, Option<[u8; 4]>, Option<Value>, Option<String>);
// Isolate the library's supported text/color prefix from typed-property data.
// This prevents its known nested-map/short-size paths from being entered, while
// genuinely delegating the metadata interpretation and UTF-8 validation.
type UserPrefix = (Option<String>, Option<[u8; 4]>, usize);
fn read_user_prefix(b: &[u8]) -> Result<UserPrefix> {
    let mut r = Reader::new(b);
    let flags = r.u32()?;
    if flags & 1 != 0 {
        let len = r.u16()? as usize;
        r.take(len)?;
    }
    if flags & 2 != 0 {
        r.take(4)?;
    }
    let end = r.pos;
    let mut prefix = b[..end].to_vec();
    prefix[..4].copy_from_slice(&(flags & 3).to_le_bytes());
    let (remaining, parsed) =
        parse_user_data_chunk(&prefix).map_err(|e| format!("Invalid user data: {e:?}"))?;
    if !remaining.is_empty() {
        return Err("Unexpected user-data prefix bytes".into());
    }
    Ok((
        parsed.text.map(str::to_owned),
        parsed.color.map(|c| [c.red, c.green, c.blue, c.alpha]),
        end,
    ))
}
fn read_user(b: &[u8]) -> Result<UserFields> {
    let flags = u32at(b, 0)?;
    let (text, color, prefix_len) = read_user_prefix(b)?;
    let text = text.unwrap_or_default();
    let mut warning = if flags & !7 != 0 {
        Some("Future user-data flags; read-only".into())
    } else {
        None
    };
    let properties = if flags & 4 != 0 {
        match read_maps(&b[prefix_len..]) {
            Ok(v) => Some(v),
            Err(e) => {
                warning = Some(format!("Properties preserved read-only: {e}"));
                None
            }
        }
    } else {
        None
    };
    Ok((text, color, properties, warning))
}
fn read_maps(b: &[u8]) -> Result<Value> {
    let mut r = Reader::new(b);
    if r.u32()? as usize != b.len() {
        return Err("Invalid property block size".into());
    }
    let n = r.count()?;
    let mut maps = Vec::new();
    for _ in 0..n {
        let key = r.u32()?;
        maps.push(json!({"key":key,"properties":read_props(&mut r,0)?}));
    }
    if r.pos != b.len() {
        return Err("Unexpected property bytes".into());
    }
    Ok(json!({"maps":maps}))
}
fn read_props(r: &mut Reader<'_>, depth: usize) -> Result<Value> {
    let n = r.count()?;
    let mut a = Vec::new();
    for _ in 0..n {
        let name = r.string()?;
        let ty = r.u16()?;
        a.push(json!({"name":name,"type":ty,"value":read_value(r,ty,depth+1)?}));
    }
    Ok(Value::Array(a))
}
fn read_value(r: &mut Reader<'_>, ty: u16, depth: usize) -> Result<Value> {
    if depth > 128 {
        return Err("Property nesting limit".into());
    }
    Ok(match ty {
        1 => json!(r.u8()? != 0),
        2 => json!(r.u8()? as i8),
        3 => json!(r.u8()?),
        4 => json!(r.u16()? as i16),
        5 => json!(r.u16()?),
        6 | 10 => json!(r.u32()? as i32),
        7 => json!(r.u32()?),
        8 => json!(i64::from_le_bytes(r.take(8)?.try_into().unwrap()).to_string()),
        9 => json!(u64::from_le_bytes(r.take(8)?.try_into().unwrap()).to_string()),
        11 => {
            let f = f32::from_bits(r.u32()?);
            if !f.is_finite() {
                return Err("Non-finite float".into());
            }
            json!(f)
        }
        12 => {
            let f = f64::from_le_bytes(r.take(8)?.try_into().unwrap());
            if !f.is_finite() {
                return Err("Non-finite float".into());
            }
            json!(f)
        }
        13 => json!(r.string()?),
        14..=16 => {
            let n = if ty == 16 { 4 } else { 2 };
            let mut a = Vec::new();
            for _ in 0..n {
                a.push(r.u32()? as i32)
            }
            json!(a)
        }
        17 => {
            let n = r.count()?;
            let et = r.u16()?;
            let mut a = Vec::new();
            for _ in 0..n {
                let t = if et == 0 { r.u16()? } else { et };
                a.push(json!({"type":t,"value":read_value(r,t,depth+1)?}));
            }
            json!({"element_type":et,"items":a})
        }
        18 => read_props(r, depth + 1)?,
        19 => json!(r
            .take(16)?
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()),
        _ => return Err(format!("Unknown type {ty}")),
    })
}
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(|| "Expected array".into())
}
fn number(v: &Value) -> Result<i64> {
    v.as_i64().ok_or_else(|| "Expected integer".into())
}
fn write_maps(v: &Value) -> Result<Vec<u8>> {
    let a = array(&v["maps"])?;
    if a.len() > MAX_ITEMS {
        return Err("Too many property maps".into());
    }
    let mut b = vec![0; 4];
    b.extend((a.len() as u32).to_le_bytes());
    for m in a {
        let key = m["key"]
            .as_u64()
            .filter(|n| *n <= u32::MAX as u64)
            .ok_or("Invalid map key")? as u32;
        b.extend(key.to_le_bytes());
        write_props(&mut b, &m["properties"], 0)?;
    }
    let len = b.len() as u32;
    b[..4].copy_from_slice(&len.to_le_bytes());
    Ok(b)
}
fn write_props(b: &mut Vec<u8>, v: &Value, depth: usize) -> Result<()> {
    let a = array(v)?;
    if a.len() > MAX_ITEMS {
        return Err("Too many properties".into());
    }
    b.extend((a.len() as u32).to_le_bytes());
    for p in a {
        strout(b, p["name"].as_str().ok_or("Expected property name")?)?;
        let t = u16::try_from(number(&p["type"])?).map_err(|_| "Invalid type")?;
        b.extend(t.to_le_bytes());
        write_value(b, t, &p["value"], depth + 1)?;
    }
    Ok(())
}
fn write_value(b: &mut Vec<u8>, ty: u16, v: &Value, depth: usize) -> Result<()> {
    if depth > 128 {
        return Err("Property nesting limit".into());
    }
    macro_rules! int {
        ($t:ty) => {{
            let n = <$t>::try_from(number(v)?).map_err(|_| "Integer out of range")?;
            b.extend(n.to_le_bytes());
        }};
    }
    match ty {
        1 => b.push(u8::from(v.as_bool().ok_or("Expected boolean")?)),
        2 => int!(i8),
        3 => int!(u8),
        4 => int!(i16),
        5 => int!(u16),
        6 | 10 => int!(i32),
        7 => int!(u32),
        8 => {
            let n = v
                .as_str()
                .ok_or("64-bit values use decimal strings")?
                .parse::<i64>()
                .map_err(|_| "Invalid int64")?;
            b.extend(n.to_le_bytes())
        }
        9 => {
            let n = v
                .as_str()
                .ok_or("64-bit values use decimal strings")?
                .parse::<u64>()
                .map_err(|_| "Invalid uint64")?;
            b.extend(n.to_le_bytes())
        }
        11 | 12 => {
            let n = v.as_f64().ok_or("Expected float")?;
            if !n.is_finite() || (ty == 11 && !(n as f32).is_finite()) {
                return Err("Float out of range".into());
            }
            if ty == 11 {
                b.extend((n as f32).to_le_bytes())
            } else {
                b.extend(n.to_le_bytes())
            }
        }
        13 => strout(b, v.as_str().ok_or("Expected string")?)?,
        14..=16 => {
            let a = array(v)?;
            if a.len() != if ty == 16 { 4 } else { 2 } {
                return Err("Invalid geometry components".into());
            }
            for p in a {
                let n = i32::try_from(number(p)?).map_err(|_| "Geometry out of range")?;
                b.extend(n.to_le_bytes())
            }
        }
        17 => {
            let a = array(&v["items"])?;
            if a.len() > MAX_ITEMS {
                return Err("Too many vector entries".into());
            }
            let et =
                u16::try_from(number(&v["element_type"])?).map_err(|_| "Invalid element type")?;
            b.extend((a.len() as u32).to_le_bytes());
            b.extend(et.to_le_bytes());
            for p in a {
                let t = u16::try_from(number(&p["type"])?).map_err(|_| "Invalid type")?;
                if et == 0 {
                    b.extend(t.to_le_bytes())
                } else if et != t {
                    return Err("Vector type mismatch".into());
                }
                write_value(b, t, &p["value"], depth + 1)?
            }
        }
        18 => write_props(b, v, depth + 1)?,
        19 => {
            let s = v.as_str().ok_or("Expected 32 hexadecimal UUID digits")?;
            if s.len() != 32 || !s.is_ascii() {
                return Err("Invalid UUID".into());
            }
            for i in (0..32).step_by(2) {
                b.push(u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "Invalid UUID")?)
            }
        }
        _ => return Err("Unknown property type".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(chunks: Vec<(u16, Vec<u8>)>) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
        for offset in [6, 8, 10] {
            b[offset..offset + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        b[12..14].copy_from_slice(&32u16.to_le_bytes());
        let mut f = vec![0; 16];
        f[4..6].copy_from_slice(&0xf1fau16.to_le_bytes());
        f[6..8].copy_from_slice(&(chunks.len() as u16).to_le_bytes());
        for (t, d) in chunks {
            f.extend(((d.len() + 6) as u32).to_le_bytes());
            f.extend(t.to_le_bytes());
            f.extend(d);
        }
        let len = f.len() as u32;
        f[..4].copy_from_slice(&len.to_le_bytes());
        b.extend(f);
        let len = b.len() as u32;
        b[..4].copy_from_slice(&len.to_le_bytes());
        b
    }
    fn layer() -> Vec<u8> {
        let mut b = vec![0; 16];
        strout(&mut b, "Layer").unwrap();
        b
    }
    fn user(s: &str) -> Vec<u8> {
        let mut b = 1u32.to_le_bytes().to_vec();
        strout(&mut b, s).unwrap();
        b
    }
    fn tags() -> Vec<u8> {
        let mut b = 2u16.to_le_bytes().to_vec();
        b.extend([0; 8]);
        for n in ["idle", "walk"] {
            b.extend([0; 17]);
            strout(&mut b, n).unwrap();
        }
        b
    }
    #[test]
    fn unchanged_is_byte_exact() {
        let bytes = fixture(vec![
            (0x2004, layer()),
            (0x2005, vec![0; 20]),
            (0xffff, vec![9, 8, 7]),
        ]);
        assert_eq!(Document::parse(&bytes).unwrap().encode().unwrap(), bytes);
    }
    #[test]
    fn global_and_layer_are_separate() {
        let bytes = fixture(vec![
            (4, vec![0, 0]),
            (0x2020, user("credits=me")),
            (0x2004, layer()),
            (0x2020, user("layer")),
        ]);
        let mut d = Document::parse(&bytes).unwrap();
        assert_eq!(d.targets()[0].text, "credits=me");
        assert_eq!(d.targets()[1].text, "layer");
        d.set_text(0, "title=Art").unwrap();
        let re = Document::parse(&d.encode().unwrap()).unwrap();
        assert_eq!(re.targets()[0].text, "title=Art");
        assert_eq!(re.targets()[1].text, "layer");
    }
    #[test]
    fn adding_global_preserves_all_art_bytes() {
        let bytes = fixture(vec![
            (0x2004, layer()),
            (0x2005, vec![0; 20]),
            (0xffff, vec![9, 8, 7]),
        ]);
        let mut d = Document::parse(&bytes).unwrap();
        let before: Vec<_> = d.frames[0]
            .chunks
            .iter()
            .map(|c| (c.kind, c.data.clone()))
            .collect();
        d.set_text(0, "license=CC0").unwrap();
        let re = Document::parse(&d.encode().unwrap()).unwrap();
        let after: Vec<_> = re.frames[0]
            .chunks
            .iter()
            .filter(|c| c.kind != 0x2020 && !(c.kind == 4 && c.data == [0, 0]))
            .map(|c| (c.kind, c.data.clone()))
            .collect();
        assert_eq!(before, after);
        assert_eq!(re.targets()[0].text, "license=CC0");
    }
    #[test]
    fn tags_keep_ordinal_and_reserved_bytes() {
        let mut d = Document::parse(&fixture(vec![(0x2018, tags())])).unwrap();
        d.set_text(2, "second tag").unwrap();
        d.set_text(1, "first tag").unwrap();
        let mut t = d.tags()[1].clone();
        t.name = "run".into();
        t.repeat = 5;
        d.set_tag(&t).unwrap();
        let re = Document::parse(&d.encode().unwrap()).unwrap();
        assert_eq!(re.targets()[1].text, "first tag");
        assert_eq!(re.targets()[2].text, "second tag");
        assert_eq!(re.tags()[1].name, "run");
        assert_eq!(re.tags()[1].repeat, 5);
    }
    #[test]
    fn typed_properties_roundtrip() {
        let p = json!({"maps":[{"key":0,"properties":[{"name":"title","type":13,"value":"Test"},{"name":"large","type":9,"value":"18446744073709551615"},{"name":"nested","type":18,"value":[{"name":"ok","type":1,"value":true}]},{"name":"vector","type":17,"value":{"element_type":3,"items":[{"type":3,"value":42}]}}]}]});
        let mut d = Document::parse(&fixture(vec![])).unwrap();
        d.set_properties(0, &p).unwrap();
        let re = Document::parse(&d.encode().unwrap()).unwrap();
        assert_eq!(re.targets()[0].properties, Some(p));
    }
    #[test]
    fn unsupported_properties_survive_text_edit() {
        let mut b = 4u32.to_le_bytes().to_vec();
        let props = vec![
            20, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 255, 255,
        ];
        b.extend(&props);
        let mut d = Document::parse(&fixture(vec![(0x2020, b)])).unwrap();
        assert!(d.targets()[0].warning.is_some());
        d.set_text(0, "safe").unwrap();
        assert!(d.frames[0].chunks[0].data.ends_with(&props));
        assert!(d.set_properties(0, &json!({"maps":[]})).is_err());
    }
    #[test]
    fn malformed_inputs_fail_without_panics() {
        let good = fixture(vec![(0x2004, layer())]);
        for end in 0..good.len() {
            assert!(Document::parse(&good[..end]).is_err());
        }
        let mut bad = good;
        bad[144..148].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Document::parse(&bad).is_err());
    }
    #[test]
    fn invalid_edits_are_rejected() {
        let mut d = Document::parse(&fixture(vec![(0x2018, tags())])).unwrap();
        let before = d.encode().unwrap();
        let mut t = d.tags()[0].clone();
        t.to = 2;
        assert!(d.set_tag(&t).is_err());
        assert!(d.set_text(0, &"x".repeat(65536)).is_err());
        assert_eq!(before, d.encode().unwrap());
    }
    #[test]
    fn independent_decoder_preserves_pixels_and_reads_global_text() {
        let mut l = layer();
        l[0] = 1;
        l[12] = 255;
        let mut cel = vec![0; 16];
        cel[6] = 255;
        cel.extend(1u16.to_le_bytes());
        cel.extend(1u16.to_le_bytes());
        cel.extend([23, 111, 209, 255]);
        let bytes = fixture(vec![(0x2004, l), (0x2005, cel), (0x2018, tags())]);
        let before = asefile::AsepriteFile::read(&bytes[..]).unwrap();
        let pixels = before.frame(0).image();
        assert_eq!(pixels.get_pixel(0, 0).0, [23, 111, 209, 255]);
        let mut d = Document::parse(&bytes).unwrap();
        d.set_text(0, "credits=Example").unwrap();
        d.set_text(1, "layer note").unwrap();
        d.set_text(2, "cel note").unwrap();
        let mut tag = d.tags()[0].clone();
        tag.name = "renamed".into();
        d.set_tag(&tag).unwrap();
        let encoded = d.encode().unwrap();
        let after = asefile::AsepriteFile::read(&encoded[..]).unwrap();
        assert_eq!(pixels, after.frame(0).image());
        assert_eq!(
            after.sprite_user_data().unwrap().text.as_deref(),
            Some("credits=Example")
        );
        assert!(after.tag_by_name("renamed").is_some());
    }
    #[test]
    fn independent_indexed_palette_unchanged() {
        let mut palette = vec![];
        palette.extend(2u32.to_le_bytes());
        palette.extend(0u32.to_le_bytes());
        palette.extend(1u32.to_le_bytes());
        palette.extend([0; 8]);
        palette.extend([0, 0, 0, 0, 0, 0]);
        palette.extend([0, 0, 23, 111, 209, 255]);
        let mut l = layer();
        l[0] = 1;
        l[12] = 255;
        let mut cel = vec![0; 16];
        cel[6] = 255;
        cel.extend(1u16.to_le_bytes());
        cel.extend(1u16.to_le_bytes());
        cel.push(1);
        let mut bytes = fixture(vec![(0x2019, palette.clone()), (0x2004, l), (0x2005, cel)]);
        bytes[12] = 8;
        let before = asefile::AsepriteFile::read(&bytes[..])
            .unwrap()
            .frame(0)
            .image();
        assert_eq!(before.get_pixel(0, 0).0, [23, 111, 209, 255]);
        let mut d = Document::parse(&bytes).unwrap();
        d.set_text(0, "indexed").unwrap();
        let output = d.encode().unwrap();
        let after = asefile::AsepriteFile::read(&output[..]).unwrap();
        assert_eq!(before, after.frame(0).image());
        assert_eq!(
            after.sprite_user_data().unwrap().text.as_deref(),
            Some("indexed")
        );
        assert_eq!(
            d.frames[0]
                .chunks
                .iter()
                .find(|c| c.kind == 0x2019)
                .unwrap()
                .data,
            palette
        );
    }
    #[test]
    fn unknown_user_flags_reject_edits_without_changes() {
        let mut b = 9u32.to_le_bytes().to_vec();
        strout(&mut b, "future").unwrap();
        b.extend([0xde, 0xad, 0xbe, 0xef]);
        let bytes = fixture(vec![(0x2020, b)]);
        let mut d = Document::parse(&bytes).unwrap();
        assert!(d.targets()[0].warning.is_some());
        assert!(d.set_text(0, "new").is_err());
        assert!(d.set_color(0, Some([1, 2, 3, 4])).is_err());
        assert!(d.set_properties(0, &json!({"maps":[]})).is_err());
        assert_eq!(d.encode().unwrap(), bytes);
    }
    #[test]
    fn library_metadata_preflight_rejects_truncation() {
        let mut userdata = user("éclair");
        userdata[0] = 3;
        userdata.extend([1, 2, 3, 4]);
        for (kind, body) in [(0x2004, layer()), (0x2018, tags()), (0x2020, userdata)] {
            for end in 0..body.len() {
                let bytes = fixture(vec![(kind, body[..end].to_vec())]);
                assert!(Document::parse(&bytes).is_err(), "kind={kind:x}, len={end}");
            }
        }
        let mut excessive = vec![255, 255];
        excessive.extend([0; 8]);
        assert!(Document::parse(&fixture(vec![(0x2018, excessive)])).is_err());
    }
    #[test]
    fn library_prefix_does_not_enter_unsupported_property_parser() {
        let mut data = vec![4, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0];
        let mut d = Document::parse(&fixture(vec![(0x2020, data.clone())])).unwrap();
        assert!(d.targets()[0].warning.is_some());
        d.set_text(0, "note").unwrap();
        assert!(d.encode().unwrap().windows(8).any(|w| w == &data[4..]));
        data[0] = 8;
        let mut future = Document::parse(&fixture(vec![(0x2020, data)])).unwrap();
        assert!(future.set_text(0, "no").is_err());
    }
}
