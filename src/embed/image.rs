//! 画像を読み込む（§16.12）。
//!
//! **ネットワークからは取得しない。** 業務の文書を開いただけで外部へ通信が出るのは
//! 受け入れられない。`http://` などを指していたら、その旨を画面に出して終わる。
//!
//! **原寸のまま持たない。** 表示上の最大幅を超える画像は縮小して保持する。
//! 4000x3000 の写真を原寸で持つと 1 枚 48MB になる。

use std::path::{Path, PathBuf};

use super::{EmbedError, EmbedSource, RenderEmbed, RenderedEmbed};

/// 画像の描画器。
pub struct ImageRenderer;

impl Default for ImageRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageRenderer {
    pub fn new() -> Self {
        Self
    }
}

/// 保持する画素数の上限（幅・高さとも）。
///
/// 表示幅に合わせて縮めたうえ、さらにこの値で頭打ちにする。
const MAX_PIXELS: u32 = 4_000;

/// ネットワークを指す参照か。
///
/// **ここで弾く。** 取得しようとしてから失敗するのではなく、
/// 最初から取りに行かない。
pub fn is_remote(reference: &str) -> bool {
    let lower = reference.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("data:")
        || lower.starts_with("//")
}

/// 文書の位置を基準にパスを解決する。
///
/// 絶対パスはそのまま。相対パスは `base` からの相対として扱う。
pub fn resolve(reference: &str, base: Option<&Path>) -> PathBuf {
    let path = Path::new(reference.trim());
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match base {
        Some(base) => base.join(path),
        None => path.to_path_buf(),
    }
}

impl RenderEmbed for ImageRenderer {
    fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
        let path = Path::new(&source.text);

        let bytes = std::fs::read(path)
            .map_err(|error| EmbedError::Failed(format!("{}: {error}", path.display())))?;

        let decoded = image::load_from_memory(&bytes)
            .map_err(|error| EmbedError::Failed(format!("読めない画像: {error}")))?;

        // **表示幅に合わせて縮める。** 原寸のまま持つとメモリを圧迫する
        let limit = (source.width.max(1.0) as u32).min(MAX_PIXELS);
        let decoded = if decoded.width() > limit {
            let height = (decoded.height() as u64 * limit as u64 / decoded.width().max(1) as u64)
                .max(1) as u32;
            decoded.resize(limit, height, image::imageops::FilterType::Triangle)
        } else {
            decoded
        };

        let rgba = decoded.to_rgba8();
        let (width, height) = rgba.dimensions();
        if width == 0 || height == 0 {
            return Err(EmbedError::Failed("寸法が 0 の画像".to_owned()));
        }

        Ok(RenderedEmbed {
            width,
            height,
            pixels: std::sync::Arc::new(rgba.into_raw()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EmbedKind;

    /// **ネットワークの参照は最初から取りに行かない。**
    #[test]
    fn remote_references_are_rejected() {
        for reference in [
            "http://example.com/a.png",
            "HTTPS://example.com/a.png",
            "ftp://example.com/a.png",
            "data:image/png;base64,AAAA",
            "//example.com/a.png",
        ] {
            assert!(is_remote(reference), "弾けていない: {reference}");
        }
    }

    #[test]
    fn local_references_are_allowed() {
        for reference in [
            "a.png",
            "./img/a.png",
            "../a.png",
            "C:/tmp/a.png",
            "/tmp/a.png",
        ] {
            assert!(!is_remote(reference), "誤って弾いた: {reference}");
        }
    }

    /// 相対パスは文書の位置を基準に解決する。
    #[test]
    fn relative_paths_resolve_against_the_document() {
        let base = Path::new("C:/docs/manual");
        assert_eq!(
            resolve("img/a.png", Some(base)),
            Path::new("C:/docs/manual/img/a.png")
        );
    }

    #[test]
    fn absolute_paths_are_kept() {
        let base = Path::new("C:/docs/manual");
        let resolved = resolve("/tmp/a.png", Some(base));
        assert!(resolved.is_absolute());
        assert!(!resolved.starts_with("C:/docs"));
    }

    #[test]
    fn missing_base_keeps_the_reference() {
        assert_eq!(resolve("a.png", None), Path::new("a.png"));
    }

    /// 1x1 の PNG を作って読ませる。
    fn tiny_png() -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("PNG を書ける");
        buffer.into_inner()
    }

    #[test]
    fn png_is_decoded() {
        let dir = std::env::temp_dir().join("mv-image-test");
        std::fs::create_dir_all(&dir).expect("作業場所を作れる");
        let path = dir.join("tiny.png");
        std::fs::write(&path, tiny_png()).expect("書ける");

        let embed = ImageRenderer::new()
            .render(&EmbedSource::new(
                EmbedKind::Image,
                path.to_string_lossy(),
                800.0,
            ))
            .expect("読める");
        assert_eq!((embed.width, embed.height), (2, 2));
        assert_eq!(embed.pixels.len(), 2 * 2 * 4);
    }

    /// **表示幅を超える画像は縮める。** 原寸のまま持たない。
    #[test]
    fn oversized_images_are_shrunk() {
        let dir = std::env::temp_dir().join("mv-image-test");
        std::fs::create_dir_all(&dir).expect("作業場所を作れる");
        let path = dir.join("wide.png");
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(400, 200, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("PNG を書ける");
        std::fs::write(&path, buffer.into_inner()).expect("書ける");

        let embed = ImageRenderer::new()
            .render(&EmbedSource::new(
                EmbedKind::Image,
                path.to_string_lossy(),
                100.0,
            ))
            .expect("読める");
        assert_eq!(embed.width, 100, "幅に合わせて縮まっていない");
        assert_eq!(embed.height, 50, "縦横比が保たれていない");
    }

    /// **失敗は握りつぶさない。** どのファイルが無いのかを出す。
    #[test]
    fn missing_file_reports_the_path() {
        let error = ImageRenderer::new()
            .render(&EmbedSource::new(
                EmbedKind::Image,
                "C:/存在しない/画像.png",
                800.0,
            ))
            .unwrap_err();
        assert!(error.to_string().contains("画像.png"), "{error}");
    }

    #[test]
    fn broken_data_reports_the_reason() {
        let dir = std::env::temp_dir().join("mv-image-test");
        std::fs::create_dir_all(&dir).expect("作業場所を作れる");
        let path = dir.join("broken.png");
        std::fs::write(&path, "これは画像ではない".as_bytes()).expect("書ける");

        let error = ImageRenderer::new()
            .render(&EmbedSource::new(
                EmbedKind::Image,
                path.to_string_lossy(),
                800.0,
            ))
            .unwrap_err();
        assert!(error.to_string().contains("読めない画像"), "{error}");
    }
}
