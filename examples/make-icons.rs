//! アイコンを作る（§7.1 / B-1）。
//!
//! `assets/*.svg` を原本に、配布で要る形式を生成する。
//!
//! ```text
//! cargo run --example make-icons
//! ```
//!
//! **新しい道具を増やさない。** 描画はアプリが使っている `resvg` で行い、
//! `.ico` の容れ物は自前で書く（中身は PNG。Vista 以降が読める）。
//! `.icns` は macOS の `iconutil` でしか作れないため、ここでは
//! その材料（`.iconset`）までを用意する。
//!
//! **生成物はリポジトリへ置かない**（同梱フォントと同じ扱い）。

use std::io::Write;
use std::path::{Path, PathBuf};

/// 出力先。
const OUT: &str = "assets/icons";

/// 小さいほうの絵へ切り替える境目（px）。
///
/// **32px 以下は細部が潰れる。** 別の絵（`icon-small.svg`）を使う
const SMALL_UNTIL: u32 = 32;

/// `.ico` に入れる大きさ。
///
/// **256 まで入れる。** エクスプローラーの「特大アイコン」が使う
const ICO_SIZES: [u32; 6] = [16, 24, 32, 48, 128, 256];

/// `.png` として出す大きさ（Linux の hicolor）。
const PNG_SIZES: [u32; 6] = [16, 24, 32, 48, 128, 256];

/// `.iconset` に入れる（名前, 大きさ）。**macOS の決まった名前**。
const ICONSET: [(&str, u32); 10] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(OUT);
    std::fs::create_dir_all(&out)?;

    let app = Art::load("assets/icon.svg", "assets/icon-small.svg")?;
    let doc = Art::load("assets/icon-doc.svg", "assets/icon-doc.svg")?;

    // --- Windows ---
    write_ico(&out.join("mdview.ico"), &app, &ICO_SIZES)?;
    write_ico(&out.join("mdview-doc.ico"), &doc, &ICO_SIZES)?;

    // --- Linux ---
    for size in PNG_SIZES {
        std::fs::write(out.join(format!("mdview-{size}.png")), app.render(size)?)?;
    }

    // --- macOS の材料 ---
    let iconset = out.join("mdview.iconset");
    std::fs::create_dir_all(&iconset)?;
    for (name, size) in ICONSET {
        std::fs::write(iconset.join(name), app.render(size)?)?;
    }

    println!("作った: {}", out.display());
    println!("  mdview.ico / mdview-doc.ico（Windows）");
    println!("  mdview-<大きさ>.png（Linux）");
    println!("  mdview.iconset（macOS。iconutil -c icns で .icns にする）");
    Ok(())
}

/// 原本の絵。大小で別の図案を持つ。
struct Art {
    large: resvg::usvg::Tree,
    small: resvg::usvg::Tree,
}

impl Art {
    fn load(large: &str, small: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            large: parse(large)?,
            small: parse(small)?,
        })
    }

    /// その大きさの PNG を作る。
    fn render(&self, size: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let tree = if size <= SMALL_UNTIL {
            &self.small
        } else {
            &self.large
        };

        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(size, size).ok_or("ピクセルマップを作れない")?;
        let source = tree.size();
        // **縦横とも同じ倍率にする。** 別々にすると図案が歪む
        let scale = size as f32 / source.width().max(source.height());
        resvg::render(
            tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        Ok(pixmap.encode_png()?)
    }
}

fn parse(path: &str) -> Result<resvg::usvg::Tree, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    Ok(resvg::usvg::Tree::from_data(
        &data,
        &resvg::usvg::Options::default(),
    )?)
}

/// `.ico` を書く。
///
/// **中身は PNG のまま入れる。** 容れ物は「6 バイトの頭 + 16 バイト × 枚数 +
/// 画像の並び」だけで、BMP へ直す必要は無い（Vista 以降が PNG を読む）。
fn write_ico(path: &Path, art: &Art, sizes: &[u32]) -> Result<(), Box<dyn std::error::Error>> {
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|size| art.render(*size).map(|png| (*size, png)))
        .collect::<Result<_, _>>()?;

    let mut out = Vec::new();
    // 頭: 予約 0 / 種別 1（アイコン）/ 枚数
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    // 画像の並びが始まる位置
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in &images {
        // **256 は 0 と書く決まり。** 1 バイトに収まらないため
        let byte = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(byte); // 幅
        out.push(byte); // 高さ
        out.push(0); // 色数（PNG なので 0）
        out.push(0); // 予約
        out.extend_from_slice(&1u16.to_le_bytes()); // 面数
        out.extend_from_slice(&32u16.to_le_bytes()); // ビット深度
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &images {
        out.extend_from_slice(png);
    }

    let mut file = std::fs::File::create(path)?;
    file.write_all(&out)?;
    Ok(())
}
