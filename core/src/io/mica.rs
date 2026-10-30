//! Mica 風の地（v2.4.0 R-05）の、壁紙の下ごしらえ。
//!
//! Mica の正体は「デスクトップの壁紙を強くぼかし、配色の色を薄く重ねたもの」。iced 0.14 は Windows で
//! 窓を透かせない（DX12 の描画先が不透明）ので、**壁紙を読んで自前でぼかして敷く**（要件定義書 R-05）。
//!
//! ここでは画像を読み、モニターの大きさに「覆う」で合わせ、**小さく縮めてから強くぼかす**
//! （大きいままぼかすと遅い。どうせ強くぼかすので、縮めても見た目は変わらない）。

use std::path::Path;

use super::backdrop::Loaded;

/// モニターの大きさに対して縮める割合（1/8）
const SHRINK: u32 = 8;
/// 縮めたあとでぼかす量（標準偏差。px）
const SIGMA: f32 = 6.0;

/// 壁紙を読み、`monitor`（画素）を覆う形にして、縮めてぼかす
pub fn prepare(path: &Path, monitor: (u32, u32)) -> Result<Loaded, String> {
    // **拡張子の無いファイルもある**（Windows 11 の TranscodedWallpaper）。中身から形式を見分ける
    let image = image::ImageReader::open(path)
        .map_err(|error| format!("壁紙を開けません（{}）: {error}", path.display()))?
        .with_guessed_format()
        .map_err(|error| format!("壁紙を読めません: {error}"))?
        .decode()
        .map_err(|error| format!("壁紙を読めません: {error}"))?;
    let width = (monitor.0 / SHRINK).max(1);
    let height = (monitor.1 / SHRINK).max(1);
    let covered = image.resize_to_fill(width, height, image::imageops::FilterType::Triangle);
    let blurred = image::imageops::fast_blur(&covered.to_rgba8(), SIGMA);
    Ok(Loaded {
        width: blurred.width(),
        height: blurred.height(),
        rgba: blurred.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// モニターの 1/8 の大きさにする（縦横比はモニターに合わせ、はみ出しは切る）
    #[test]
    fn the_wallpaper_is_shrunk_to_cover_the_monitor() {
        let dir = std::env::temp_dir().join(format!("mdview-mica-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 拡張子無し（TranscodedWallpaper と同じ）
        let path = dir.join("wallpaper");
        let image = image::RgbaImage::from_pixel(400, 400, image::Rgba([10, 20, 30, 255]));
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::fs::File::create(&path).unwrap(),
                image::ImageFormat::Png,
            )
            .unwrap();
        let loaded = prepare(&path, (1920, 1080)).expect("読める");
        assert_eq!((loaded.width, loaded.height), (240, 135));
        assert_eq!(&loaded.rgba[..3], &[10, 20, 30], "単色はぼかしても同じ色");
        let _ = std::fs::remove_dir_all(dir);
        assert!(prepare(Path::new("無い.png"), (100, 100)).is_err());
    }
}
