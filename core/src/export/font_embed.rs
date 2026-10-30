//! PPTX に埋め込む字体を作る（v2.8.0 R-04）。**使った字だけ**を残した同梱フォントを、EOT（Embedded OpenType）に包む。
//!
//! **字形の番号は変えない。** 使わない字形の中身（`glyf`）を空にし、字と字形の対応（`cmap`）を使った字だけで作り直す。
//! 番号を振り直さないので、幅（`hmtx`）・字の大きさの表はそのまま使える。PDF の書き出しの抜き出し（subsetter）は
//! `cmap` を落とすので、PowerPoint には使えない。
//!
//! 縦書き・字の置き換え・字の名前の表（`vhea` `vmtx` `GSUB` `GPOS` `GDEF` `BASE` `post` の名前・`DSIG` `meta`）は落とす。
//! PowerPoint は **圧縮しない EOT（版 0x00020001・印 0）** を `.fntdata` として読む（MicroType Express で縮めない）。
//!
//! 同梱フォント（PlemolJP・IBM Plex Sans JP）は SIL Open Font License 1.1 で、埋め込みを許している（`OS/2` の `fsType` が 0）。
//! 名前の表（著作権表示・ライセンス）はそのまま残す。**画面も文書も知らない**（バイト列を受けて返すだけ）。

use std::collections::{BTreeMap, BTreeSet};

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// 字体の表の一覧（名前 → 中身）
fn tables(font: &[u8]) -> Option<BTreeMap<[u8; 4], &[u8]>> {
    let count = u16_at(font, 4)? as usize;
    let mut out = BTreeMap::new();
    for index in 0..count {
        let record = 12 + index * 16;
        let tag: [u8; 4] = font.get(record..record + 4)?.try_into().ok()?;
        let offset = u32_at(font, record + 8)? as usize;
        let length = u32_at(font, record + 12)? as usize;
        out.insert(tag, font.get(offset..offset + length)?);
    }
    Some(out)
}

/// 表の検査の和（4 バイトずつ足す）
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// 表を並べて字体にする（名前の順・4 バイトに揃える。`head` の `checkSumAdjustment` を入れ直す）
fn assemble(tables: &BTreeMap<[u8; 4], Vec<u8>>) -> Vec<u8> {
    let count = tables.len() as u16;
    let mut power = 1u16;
    let mut selector = 0u16;
    while power * 2 <= count {
        power *= 2;
        selector += 1;
    }
    let search = power * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&search.to_be_bytes());
    out.extend_from_slice(&selector.to_be_bytes());
    out.extend_from_slice(&(count * 16 - search).to_be_bytes());
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    let mut head_at = None;
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = Some(offset);
        }
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + tables.len() * 16 + body.len();
    }
    out.extend_from_slice(&body);
    if let Some(head) = head_at {
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    out
}

/// 字形が組み合わせ（他の字形を参照する）なら、参照する字形の番号
fn components(glyph: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    if glyph.len() < 10 || (u16_at(glyph, 0).unwrap_or(0) as i16) >= 0 {
        return out;
    }
    let mut at = 10;
    while let (Some(flags), Some(index)) = (u16_at(glyph, at), u16_at(glyph, at + 2)) {
        out.push(index);
        at += 4;
        at += if flags & 0x0001 != 0 { 4 } else { 2 };
        if flags & 0x0008 != 0 {
            at += 2;
        } else if flags & 0x0040 != 0 {
            at += 4;
        } else if flags & 0x0080 != 0 {
            at += 8;
        }
        if flags & 0x0020 == 0 {
            break;
        }
    }
    out
}

/// 字と字形の対応（形式 4 と 12）を作る
fn cmap(mapping: &BTreeMap<u32, u16>) -> Vec<u8> {
    // 形式 4（基本多言語面）。番号の差が同じ字の連なりを 1 つの区間にする
    let mut segments: Vec<(u16, u16, i32)> = Vec::new();
    for (&code, &glyph) in mapping.range(..0x10000) {
        let code = code as u16;
        let delta = glyph as i32 - code as i32;
        match segments.last_mut() {
            Some((_, end, last)) if *end + 1 == code && *last == delta => *end = code,
            _ => segments.push((code, code, delta)),
        }
    }
    segments.push((0xFFFF, 0xFFFF, 1));
    let count = segments.len() as u16;
    let mut power = 1u16;
    let mut selector = 0u16;
    while power * 2 <= count {
        power *= 2;
        selector += 1;
    }
    let mut format4 = Vec::new();
    let length = 16 + segments.len() * 8;
    for value in [
        4u16,
        length as u16,
        0,
        count * 2,
        power * 2,
        selector,
        count * 2 - power * 2,
    ] {
        format4.extend_from_slice(&value.to_be_bytes());
    }
    for (_, end, _) in &segments {
        format4.extend_from_slice(&end.to_be_bytes());
    }
    format4.extend_from_slice(&0u16.to_be_bytes());
    for (start, _, _) in &segments {
        format4.extend_from_slice(&start.to_be_bytes());
    }
    for (_, _, delta) in &segments {
        format4.extend_from_slice(&(*delta as u16).to_be_bytes());
    }
    for _ in &segments {
        format4.extend_from_slice(&0u16.to_be_bytes());
    }
    // 形式 12（全部の面）
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (&code, &glyph) in mapping {
        let glyph = glyph as u32;
        match groups.last_mut() {
            Some((start, end, first)) if *end + 1 == code && *first + (code - *start) == glyph => {
                *end = code
            }
            _ => groups.push((code, code, glyph)),
        }
    }
    let mut format12 = Vec::new();
    format12.extend_from_slice(&12u16.to_be_bytes());
    format12.extend_from_slice(&0u16.to_be_bytes());
    format12.extend_from_slice(&((16 + groups.len() * 12) as u32).to_be_bytes());
    format12.extend_from_slice(&0u32.to_be_bytes());
    format12.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (start, end, glyph) in &groups {
        for value in [start, end, glyph] {
            format12.extend_from_slice(&value.to_be_bytes());
        }
    }
    // 表の頭（版 0・2 つ: Windows の BMP と、Windows の全部）
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    let first = 4 + 2 * 8;
    for (encoding, offset) in [(1u16, first), (10u16, first + format4.len())] {
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&encoding.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    out.extend_from_slice(&format4);
    out.extend_from_slice(&format12);
    out
}

/// 使った字だけを残した字体（TrueType）。字体が読めなければ理由
pub fn subset(font: &[u8], chars: &BTreeSet<char>) -> Result<Vec<u8>, String> {
    let face =
        ttf_parser::Face::parse(font, 0).map_err(|error| format!("字体を読めない: {error}"))?;
    let tables = tables(font).ok_or("字体の表を読めない")?;
    let get = |tag: &[u8; 4]| {
        tables
            .get(tag)
            .copied()
            .ok_or(format!("{} の表が無い", String::from_utf8_lossy(tag)))
    };
    let head = get(b"head")?;
    let loca = get(b"loca")?;
    let glyf = get(b"glyf")?;
    let count = face.number_of_glyphs() as usize;
    let long = u16_at(head, 50).ok_or("head が短い")? == 1;
    let offset = |glyph: usize| -> Option<usize> {
        if long {
            u32_at(loca, glyph * 4).map(|value| value as usize)
        } else {
            u16_at(loca, glyph * 2).map(|value| value as usize * 2)
        }
    };
    let glyph_data = |glyph: usize| -> &[u8] {
        match (offset(glyph), offset(glyph + 1)) {
            (Some(start), Some(end)) if start <= end && end <= glyf.len() => &glyf[start..end],
            _ => &[],
        }
    };
    // 残す字形（.notdef と、使った字と、組み合わせが参照するもの）
    let mut mapping = BTreeMap::new();
    let mut keep = BTreeSet::from([0u16]);
    for &c in chars {
        if let Some(glyph) = face.glyph_index(c) {
            mapping.insert(c as u32, glyph.0);
            keep.insert(glyph.0);
        }
    }
    let mut pending: Vec<u16> = keep.iter().copied().collect();
    while let Some(glyph) = pending.pop() {
        for part in components(glyph_data(glyph as usize)) {
            if keep.insert(part) {
                pending.push(part);
            }
        }
    }
    // 字形の中身（使わないものは空）と、長い形の位置の表
    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((count + 1) * 4);
    for glyph in 0..count {
        new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
        if keep.contains(&(glyph as u16)) {
            new_glyf.extend_from_slice(glyph_data(glyph));
            while new_glyf.len() % 4 != 0 {
                new_glyf.push(0);
            }
        }
    }
    new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
    let mut new_head = head.to_vec();
    new_head[8..12].copy_from_slice(&[0; 4]);
    new_head[50..52].copy_from_slice(&1u16.to_be_bytes());
    // 字の名前は落とす（版 3）
    let mut post = get(b"post")?.get(..32).ok_or("post が短い")?.to_vec();
    post[..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());

    const DROP: [&[u8; 4]; 9] = [
        b"DSIG", b"meta", b"GSUB", b"GPOS", b"GDEF", b"BASE", b"vhea", b"vmtx", b"VORG",
    ];
    let mut out: BTreeMap<[u8; 4], Vec<u8>> = BTreeMap::new();
    for (tag, data) in &tables {
        if DROP.contains(&tag) {
            continue;
        }
        out.insert(*tag, data.to_vec());
    }
    out.insert(*b"glyf", new_glyf);
    out.insert(*b"loca", new_loca);
    out.insert(*b"head", new_head);
    out.insert(*b"cmap", cmap(&mapping));
    out.insert(*b"post", post);
    Ok(assemble(&out))
}

/// 名前の表から 1 つ（Windows・英語を先に。UTF-16LE のバイト列で返す）
fn name_utf16le(font: &[u8], id: u16) -> Vec<u8> {
    let Some(face) = ttf_parser::Face::parse(font, 0).ok() else {
        return Vec::new();
    };
    let names = face.names();
    let found = names
        .into_iter()
        .filter(|name| name.name_id == id && name.is_unicode())
        .max_by_key(|name| {
            (
                name.platform_id == ttf_parser::PlatformId::Windows,
                name.language_id == 0x409,
            )
        })
        .and_then(|name| name.to_string());
    found
        .unwrap_or_default()
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// 字体を EOT（圧縮しない。版 0x00020001）に包む
pub fn eot(font: &[u8]) -> Result<Vec<u8>, String> {
    let tables = tables(font).ok_or("字体の表を読めない")?;
    let os2 = tables.get(b"OS/2").copied().ok_or("OS/2 の表が無い")?;
    let head = tables.get(b"head").copied().ok_or("head の表が無い")?;
    let mut out = Vec::new();
    let push32 = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    let push16 = |out: &mut Vec<u8>, value: u16| out.extend_from_slice(&value.to_le_bytes());
    push32(&mut out, 0); // EOTSize（後で入れる）
    push32(&mut out, font.len() as u32);
    push32(&mut out, 0x0002_0001);
    push32(&mut out, 0); // 印: 縮めない・XOR しない
    out.extend_from_slice(os2.get(32..42).ok_or("OS/2 が短い")?); // PANOSE
    out.push(1); // DEFAULT_CHARSET
    out.push(u8::from(u16_at(os2, 62).unwrap_or(0) & 1 != 0)); // 斜体
    push32(&mut out, u16_at(os2, 4).unwrap_or(400) as u32); // 太さ
    push16(&mut out, u16_at(os2, 8).unwrap_or(0)); // fsType
    push16(&mut out, 0x504C); // 印の数
    for at in [42, 46, 50, 54] {
        push32(&mut out, u32_at(os2, at).unwrap_or(0)); // UnicodeRange1〜4
    }
    for at in [78, 82] {
        push32(&mut out, u32_at(os2, at).unwrap_or(0)); // CodePageRange1〜2
    }
    push32(&mut out, u32_at(head, 8).unwrap_or(0)); // CheckSumAdjustment
    for _ in 0..4 {
        push32(&mut out, 0); // Reserved1〜4
    }
    for id in [1u16, 2, 5, 4] {
        push16(&mut out, 0); // Padding
        let name = name_utf16le(font, id);
        push16(&mut out, name.len() as u16);
        out.extend_from_slice(&name);
    }
    push16(&mut out, 0); // Padding5
    push16(&mut out, 0); // RootStringSize（どこでも使える）
    out.extend_from_slice(font);
    let size = out.len() as u32;
    out[..4].copy_from_slice(&size.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> &'static [u8] {
        crate::render::fonts::EMBEDDED[2]
    }

    /// 使った字だけが字形を持ち、対応表は使った字だけ。字形の番号と幅は変わらない
    #[test]
    fn subsets_keep_only_used_glyphs() {
        let chars: BTreeSet<char> = "題名 Abc。".chars().collect();
        let small = subset(body(), &chars).expect("抜き出せる");
        assert!(small.len() < 600 * 1024, "{} バイト", small.len());
        let original = ttf_parser::Face::parse(body(), 0).unwrap();
        let face = ttf_parser::Face::parse(&small, 0).expect("抜き出したものも読める");
        assert_eq!(
            face.number_of_glyphs(),
            original.number_of_glyphs(),
            "番号は変えない"
        );
        for c in chars.iter().copied().filter(|c| *c != ' ') {
            let glyph = face.glyph_index(c).expect("使った字は引ける");
            assert_eq!(Some(glyph), original.glyph_index(c));
            assert!(face.glyph_bounding_box(glyph).is_some(), "{c} の形がある");
            assert_eq!(
                face.glyph_hor_advance(glyph),
                original.glyph_hor_advance(glyph)
            );
        }
        assert!(face.glyph_index('字').is_none(), "使っていない字は引けない");
        let unused = original.glyph_index('字').unwrap();
        assert!(
            face.glyph_bounding_box(unused).is_none(),
            "使っていない字形は空"
        );
        // 検査の和がそろっている（head の調整値）
        assert_eq!(checksum(&small), 0xB1B0_AFBA);
    }

    #[test]
    fn eot_wraps_the_font() {
        let chars: BTreeSet<char> = "a".chars().collect();
        let small = subset(body(), &chars).unwrap();
        let wrapped = eot(&small).unwrap();
        let le32 = |at: usize| u32::from_le_bytes(wrapped[at..at + 4].try_into().unwrap());
        assert_eq!(le32(0) as usize, wrapped.len());
        assert_eq!(le32(4) as usize, small.len());
        assert_eq!(le32(8), 0x0002_0001);
        assert_eq!(le32(12), 0, "縮めない");
        assert_eq!(u16::from_le_bytes([wrapped[34], wrapped[35]]), 0x504C);
        assert!(wrapped.ends_with(&small));
        let family: Vec<u8> = "IBM Plex Sans JP"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(wrapped
            .windows(family.len())
            .any(|window| window == family.as_slice()));
    }
}
