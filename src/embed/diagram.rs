//! Mermaid の図を描く（設計メモ DEC-207 / §7.3）。
//!
//! merman で SVG を作り、resvg で画素へ落とす。
//!
//! **落とし穴が 2 つある。どちらも PoC で実際に踏んだ。**
//!
//! 1. merman は既定でラベルを `<foreignObject>`（HTML）で出す。
//!    **resvg は foreignObject を描けない**ため、ラスタライズは「成功」するのに
//!    ラベルの文字だけが消える。寸法は返るので気づきにくい。
//!    → サイト設定で一括無効化する。
//! 2. `usvg::Options::default()` のフォントデータベースは**空**である。
//!    そのままでは `<text>` が一切描かれない。
//!    → 同梱フォント（DEC-209）を登録する。

use std::sync::OnceLock;

use merman::{Engine, MermaidConfig, OperationControl, RenderOutput, RenderRequest, Renderer};
use resvg::usvg;

use super::{EmbedError, EmbedSource, RenderEmbed, RenderedEmbed};

/// 図の描画器。
pub struct DiagramRenderer {
    renderer: Renderer,
}

impl Default for DiagramRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagramRenderer {
    pub fn new() -> Self {
        Self {
            renderer: Renderer::new().with_engine(Engine::new().with_site_config(site_config())),
        }
    }
}

/// HTML ラベルを一括で切る設定（§7.3）。
///
/// `%%{init: ...}%%` を文書ごとに書かせるのは現実的でないため、アプリ側で切る。
fn site_config() -> MermaidConfig {
    MermaidConfig::from_value(serde_json::json!({
        "htmlLabels": false,
        // **書体も指定する。** 既定は `"trebuchet ms", verdana, arial, sans-serif` で、
        // 同梱フォントに無い名前ばかりになる（DEC-209）
        "fontFamily": crate::render::fonts::BODY,
        "flowchart": { "htmlLabels": false },
        "class": { "htmlLabels": false },
        "state": { "htmlLabels": false },
        "er": { "htmlLabels": false },
    }))
}

/// resvg に渡す設定。**同梱フォントを登録する。**
///
/// システムフォントを読むと、日本語に中国語の字形が拾われる問題が戻る
/// （§6.5.5 / DEC-208）。エディタ・プレビューと図で字形を揃えるため、
/// 本文と同じ IBM Plex Sans JP を既定にする。
fn svg_options() -> &'static usvg::Options<'static> {
    static OPTIONS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut options = usvg::Options::default();
        {
            let db = options.fontdb_mut();
            for bytes in crate::render::fonts::EMBEDDED {
                db.load_font_data(bytes.to_vec());
            }
            // **総称名を同梱フォントへ向ける。** SVG が `sans-serif` のような
            // 総称名で終わると usvg はここを見る。既定は "Arial" などで、
            // 同梱だけのデータベースには無いため**字形が 1 つも引けない**
            // （ラベルが消えて枠線だけになる。実際に踏んだ）
            let body = crate::render::fonts::BODY.to_owned();
            db.set_sans_serif_family(body.clone());
            db.set_serif_family(body.clone());
            db.set_cursive_family(body.clone());
            db.set_fantasy_family(body);
            db.set_monospace_family(crate::render::fonts::MONO.to_owned());
        }
        options.font_family = crate::render::fonts::BODY.to_owned();
        options.font_size = 14.0;
        options
    })
}

/// 図の高さの上限（px）。
///
/// **青天井にしない。** gantt のような横長の図は原寸が数千 px になり得る。
/// レイアウトは幅に合わせて縮めるが、画素そのものを持つとメモリを食う。
const MAX_PIXELS: u32 = 4_000;

/// この図を描くか（設計メモ OPEN-210）。
///
/// **erDiagram は描かない。** merman は `er.htmlLabels: false` を渡しても
/// foreignObject を残すため、ラベルが本文として出てこない。
/// **枠線だけの図を出すより原文を見せるほうがよい。**
///
/// **画面・PDF・HTML で同じ判断をする。** 出力ごとに書くと、いまのように
/// HTML だけラベルの欠けた図が出る（利用者の報告。§10.30）
pub fn is_supported(text: &str) -> bool {
    !text.trim_start().starts_with("erDiagram")
}

impl DiagramRenderer {
    /// 図の SVG。**HTML 出力はこれをそのまま埋める**（§17A.2）。
    ///
    /// 画面と PDF は画素へ落とすが、HTML はブラウザが描くので落とす必要が無い。
    /// 落とすと拡大に耐えなくなる
    pub fn svg(&self, text: &str) -> Result<String, EmbedError> {
        let output = self
            .renderer
            .render(RenderRequest::svg(
                text,
                OperationControl::new(),
                Default::default(),
            ))
            .map_err(|error| EmbedError::Failed(format!("{error}")))?;

        let RenderOutput::Svg(Some(output)) = output else {
            return Err(EmbedError::Failed("SVG が返らなかった".to_owned()));
        };
        Ok(output.svg().to_owned())
    }
}

impl RenderEmbed for DiagramRenderer {
    fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
        rasterize(&self.svg(&source.text)?)
    }
}

/// SVG を画素へ落とす。
pub(crate) fn rasterize(svg: &str) -> Result<RenderedEmbed, EmbedError> {
    rasterize_with(svg, svg_options())
}

fn rasterize_with(svg: &str, options: &usvg::Options) -> Result<RenderedEmbed, EmbedError> {
    let tree = usvg::Tree::from_str(svg, options)
        .map_err(|error| EmbedError::Failed(format!("SVG を読めない: {error}")))?;

    let size = tree.size().to_int_size();
    let (width, height) = (size.width(), size.height());
    if width == 0 || height == 0 {
        return Err(EmbedError::Failed("寸法が 0 の図".to_owned()));
    }
    if width > MAX_PIXELS || height > MAX_PIXELS {
        return Err(EmbedError::Failed(format!(
            "図が大きすぎる（{width}x{height}）"
        )));
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| EmbedError::Failed("画素を確保できない".to_owned()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    Ok(RenderedEmbed {
        width,
        height,
        pixels: std::sync::Arc::new(pixmap.take()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EmbedKind;

    /// **暗い画素**を数える（§7.3 と同じ指標）。
    ///
    /// **透明でない画素を数えてはいけない。** merman の SVG は
    /// `background-color: white` で全面を塗るため、字形が 1 つも無くても
    /// 画素数は常に「幅 × 高さ」になる。最初そう書いて、**何も検出できない
    /// 指標で測った**（実際に踏んだ）。
    fn dark_pixels(embed: &RenderedEmbed) -> usize {
        embed
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 0 && (px[0] as u16 + px[1] as u16 + px[2] as u16) < 384)
            .count()
    }

    fn render(source: &str) -> RenderedEmbed {
        DiagramRenderer::new()
            .render(&EmbedSource::new(EmbedKind::Diagram, source, 800.0))
            .expect("描画できる")
    }

    #[test]
    fn flowchart_is_drawn() {
        let embed = render("graph TD\n    A[開始] --> B[終了]\n");
        assert!(embed.width > 0 && embed.height > 0);
        assert!(
            dark_pixels(&embed) > 200,
            "中身が空: {}",
            dark_pixels(&embed)
        );
    }

    /// **日本語のラベルが描かれること。**
    ///
    /// foreignObject が残っていると、枠線と矢印だけで文字が消える。
    /// PoC では既定設定で 278 画素（枠線のみ）、一括無効化で 1,649 画素だった。
    #[test]
    fn japanese_labels_are_drawn() {
        let (with_fonts, without_fonts) =
            glyph_pixels("graph TD\n    A[日本語のラベル] --> B[もうひとつ]\n");
        assert!(
            with_fonts > without_fonts,
            "字形が 1 つも描かれていない: {with_fonts} vs {without_fonts}"
        );
    }

    #[test]
    fn ascii_labels_are_drawn() {
        let (with_fonts, without_fonts) = glyph_pixels("graph TD\n    A[Start] --> B[End]\n");
        assert!(
            with_fonts > without_fonts,
            "字形が 1 つも描かれていない: {with_fonts} vs {without_fonts}"
        );
    }

    /// 同じ SVG を「同梱フォントあり」と「フォント無し」で描き比べる。
    /// 差は**字形の画素だけ**である（図形は同じなので）。
    ///
    /// **ラベルの有無で比べてはいけない。** merman はラベルの文字数から
    /// ノードの幅を決めるため、文字が 1 つも描かれなくても画素数は大きく変わる。
    /// 最初そう書いて、**文字が消えているのに通る試験**を作った（実際に踏んだ）。
    fn glyph_pixels(source: &str) -> (usize, usize) {
        let renderer = Renderer::new().with_engine(Engine::new().with_site_config(site_config()));
        let Ok(RenderOutput::Svg(Some(output))) = renderer.render(RenderRequest::svg(
            source,
            OperationControl::new(),
            Default::default(),
        )) else {
            panic!("SVG を作れない");
        };
        let svg = output.svg();

        let with_fonts = rasterize_with(svg, svg_options()).expect("描画できる");
        // フォントを 1 つも持たないデータベース = 字形が引けない
        let bare = usvg::Options::default();
        let without_fonts = rasterize_with(svg, &bare).expect("描画できる");
        (dark_pixels(&with_fonts), dark_pixels(&without_fonts))
    }

    #[test]
    fn sequence_diagram_is_drawn() {
        let embed = render("sequenceDiagram\n    A->>B: こんにちは\n    B-->>A: やあ\n");
        assert!(dark_pixels(&embed) > 500);
    }

    #[test]
    fn pie_is_drawn() {
        let embed = render("pie\n    \"い\" : 40\n    \"ろ\" : 60\n");
        assert!(dark_pixels(&embed) > 500);
    }

    /// **構文誤りは失敗として返す。** 黙って空の図を出さない。
    #[test]
    fn broken_source_fails() {
        let result = DiagramRenderer::new().render(&EmbedSource::new(
            EmbedKind::Diagram,
            "これは mermaid ではない",
            800.0,
        ));
        assert!(result.is_err(), "誤った入力が成功した");
    }

    /// 画素は RGBA8 で、寸法ぶんある。
    #[test]
    fn pixels_are_rgba8() {
        let embed = render("graph TD\n    A --> B\n");
        assert_eq!(
            embed.pixels.len(),
            embed.width as usize * embed.height as usize * 4
        );
    }
}

#[cfg(test)]
mod open_210 {
    use super::*;

    /// **OPEN-210 の再現確認**（§7.3）。
    ///
    /// erDiagram だけ `er.htmlLabels: false` を渡しても foreignObject が
    /// 12 箇所残り、ラベルが欠けていた。いまも同じかを測る。
    #[test]
    #[ignore]
    fn report_foreign_objects() {
        let renderer = Renderer::new().with_engine(Engine::new().with_site_config(site_config()));
        let cases = [
            (
                "flowchart",
                "graph TD
    A[開始] --> B[終了]
",
            ),
            (
                "sequence",
                "sequenceDiagram
    A->>B: こんにちは
",
            ),
            (
                "class",
                "classDiagram
    class 顧客 {
      +名前
    }
",
            ),
            (
                "state",
                "stateDiagram-v2
    [*] --> 待機
    待機 --> 完了
",
            ),
            (
                "er",
                "erDiagram
    顧客 ||--o{ 注文 : \"持つ\"
",
            ),
            (
                "gantt",
                "gantt
    title 予定
    section A
    作業 :a1, 2026-01-01, 3d
",
            ),
            (
                "pie",
                "pie
    \"い\" : 40
    \"ろ\" : 60
",
            ),
        ];
        for (label, source) in cases {
            let Ok(RenderOutput::Svg(Some(output))) = renderer.render(RenderRequest::svg(
                source,
                OperationControl::new(),
                Default::default(),
            )) else {
                println!("{label:<10} | 描画に失敗");
                continue;
            };
            let svg = output.svg();
            let foreign = svg.matches("foreignObject").count();
            let texts = svg.matches("<text").count();
            let drawn = rasterize(svg).map(|e| {
                e.pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] > 0)
                    .count()
            });
            println!(
                "{label:<10} | foreignObject {foreign:>2} | <text> {texts:>2} | 描画画素 {:?}",
                drawn
            );
        }
    }
}
