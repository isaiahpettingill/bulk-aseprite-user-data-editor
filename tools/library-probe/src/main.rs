use aseprite::{AsepriteFile, UserData};
fn fixture(chunks: Vec<(u16, Vec<u8>)>) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
    for offset in [6, 8, 10] {
        b[offset..offset + 2].copy_from_slice(&1u16.to_le_bytes());
    }
    b[12..14].copy_from_slice(&32u16.to_le_bytes());
    b[34] = 1;
    b[35] = 1;
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
fn chunks(b: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let mut p = 144;
    let mut out = vec![];
    while p < b.len() {
        let size = u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize;
        let kind = u16::from_le_bytes(b[p + 4..p + 6].try_into().unwrap());
        out.push((kind, b[p + 6..p + size].to_vec()));
        p += size;
    }
    out
}
fn main() {
    let mut layer = vec![0; 16];
    layer[0] = 1;
    layer[12] = 255;
    layer.extend([1, 0, b'L']);
    let no_data = fixture(vec![
        (4, vec![0, 0]),
        (0x2004, layer.clone()),
        (0x7777, vec![9, 8, 7]),
    ]);
    let mut file = AsepriteFile::from_reader(&no_data[..]).unwrap();
    file.set_sprite_user_data(UserData {
        text: Some("new credits".into()),
        ..Default::default()
    });
    let mut out = vec![];
    file.write_to(&mut out).unwrap();
    let reread = AsepriteFile::from_reader(&out[..]).unwrap();
    println!(
        "new sprite userdata persists: {}",
        reread.sprite_user_data().is_some()
    );
    let mut ud = 1u32.to_le_bytes().to_vec();
    ud.extend([3, 0, b'o', b'l', b'd']);
    let mut cel = vec![0; 16];
    cel[6] = 255;
    cel[7] = 2;
    cel.extend([1, 0, 1, 0]);
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::best());
    z.write_all(&[23, 111, 209, 255]).unwrap();
    cel.extend(z.finish().unwrap());
    cel[11] = 0xab;
    let before = fixture(vec![
        (4, vec![0, 0]),
        (0x2020, ud),
        (0x7777, vec![9, 8, 7]),
        (0x2004, layer),
        (0x2005, cel.clone()),
    ]);
    let mut f = AsepriteFile::from_reader(&before[..]).unwrap();
    f.set_sprite_user_data(UserData {
        text: Some("edited".into()),
        ..Default::default()
    });
    let mut after = vec![];
    f.write_to(&mut after).unwrap();
    let ac = chunks(&after);
    let output_cel = &ac.iter().find(|c| c.0 == 0x2005).unwrap().1;
    println!(
        "existing userdata persists: {:?}",
        AsepriteFile::from_reader(&after[..])
            .unwrap()
            .sprite_user_data()
    );
    println!(
        "unknown chunk preserved: {}",
        ac.contains(&(0x7777, vec![9, 8, 7]))
    );
    println!(
        "compressed bytes preserved: {}",
        cel[20..] == output_cel[20..]
    );
    println!(
        "entire cel payload preserved (including reserved): {}",
        cel == *output_cel
    );
    let mut tiny = fixture(vec![(0x7777, vec![])]);
    tiny[144..148].copy_from_slice(&5u32.to_le_bytes());
    println!(
        "invalid short chunk panics: {}",
        std::panic::catch_unwind(|| AsepriteFile::from_reader(&tiny[..])).is_err()
    );
}
