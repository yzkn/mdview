//! 印に使う字の幅を実測する（§4.11 / §10.50）。
//!
//! ```text
//! cargo run --example check-marks
//! ```
//!
//! **重ねる印は、半角に描かれる字でなければならない。** 印だけを並べた
//! 行を本文の上へ重ねるため、1 つでも幅の違う字が混ざると、その先の印が
//! まるごとずれる（実際に `□` と `→` でずれた）。
//!
//! 曖昧幅（East Asian Ambiguous）の字は、和文フォントでは全角に描かれる
//! ことが多い。**見た目で判断せず、同梱フォントの送り幅を測る。**

use ttf_parser::Face;

/// 本文に使う等幅フォント（`render::fonts::EMBEDDED` の 1 本目）。
const MONO: &[u8] = include_bytes!("../assets/fonts/PlemolJP-Regular.ttf");

/// 測る字。**印に使っているもの + 候補**。
const CANDIDATES: [(char, &str); 12] = [
    (' ', "半角スペース（基準）"),
    ('·', "中黒（空白の印）"),
    ('→', "右矢印（タブの印）"),
    ('↵', "折り返し矢印（CRLF の印）"),
    ('↓', "下矢印（LF の印）"),
    ('□', "白四角（全角スペースの印・不採用）"),
    ('>', "大なり（タブの代わりの候補）"),
    ('»', "二重山括弧（候補）"),
    ('¶', "段落記号（候補）"),
    ('^', "ハット（候補）"),
    ('\u{3000}', "全角スペース（全角の基準）"),
    ('あ', "ひらがな（全角の基準）"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let face = Face::parse(MONO, 0)?;
    let em = face.units_per_em() as f32;

    let narrow = advance(&face, ' ').ok_or("半角スペースが無い")? as f32;

    println!("フォント: PlemolJP-Regular（1em = {em} 単位）");
    println!("半角の送り幅 = {narrow} 単位\n");
    println!(
        "{:<4} {:>8} {:>6}  {:<10} 用途",
        "字", "送り幅", "半角比", "判定"
    );

    let mut bad = Vec::new();
    for (ch, note) in CANDIDATES {
        let Some(width) = advance(&face, ch) else {
            println!("{ch:<4} {:>8} {:>6}  {:<10} {note}", "—", "—", "字が無い");
            bad.push((ch, note));
            continue;
        };
        let ratio = width as f32 / narrow;
        // **ぴったり 1 倍のものだけ使える。** 0.99 や 1.01 でも行がずれていく
        let ok = (ratio - 1.0).abs() < 0.001;
        let verdict = if ok { "使える" } else { "使えない" };
        println!("{ch:<4} {width:>8} {ratio:>6.2}  {verdict:<10} {note}");
        if !ok && ch != '\u{3000}' && ch != 'あ' {
            bad.push((ch, note));
        }
    }

    println!();
    if bad.is_empty() {
        println!("候補はすべて半角。印として使える");
    } else {
        println!("**半角でない字**（印に使うとその先がずれる）:");
        for (ch, note) in &bad {
            println!("  {ch}  {note}");
        }
    }
    Ok(())
}

/// その字の送り幅（フォントの単位）。
fn advance(face: &Face<'_>, ch: char) -> Option<u16> {
    let id = face.glyph_index(ch)?;
    face.glyph_hor_advance(id)
}
