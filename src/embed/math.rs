//! LaTeX の数式を描く（設計メモ DEC-210 / §7.4）。
//!
//! latex-rust で SVG を作り、[`super::diagram::rasterize`] で画素へ落とす。
//!
//! **図と違って外部フォントが要らない。** latex-rust の出力はすべて `<path>` で、
//! `<text>` は 1 つも無い。数式はアウトライン化されて出るため、
//! 画面・PDF・HTML のどの経路でも同じ結果になる（§7.4）。
//!
//! 数式用フォント（STIX Two Math）はクレートに同梱されている。

use std::sync::OnceLock;

use latex_rust::{latex_to_svg, MathFont, SvgOptions};

use super::{EmbedError, EmbedSource, RenderEmbed, RenderedEmbed};

/// 数式の描画器。
pub struct MathRenderer;

impl Default for MathRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl MathRenderer {
    pub fn new() -> Self {
        Self
    }
}

/// 数式用フォント。**1 度だけ読む。**
///
/// クレート同梱の STIX Two Math。読み込みに失敗したら、その理由を
/// 描画のたびに返す（黙って何も出さない状態にしない）。
fn math_font() -> Result<&'static MathFont, String> {
    static FONT: OnceLock<Result<MathFont, String>> = OnceLock::new();
    FONT.get_or_init(|| MathFont::stix_two_math().map_err(|error| format!("{error}")))
        .as_ref()
        .map_err(|reason| reason.clone())
}

impl MathRenderer {
    /// 数式の SVG。**HTML 出力はこれをそのまま埋める**（§17A.2）。
    pub fn svg(&self, text: &str) -> Result<String, EmbedError> {
        let font = math_font().map_err(EmbedError::Failed)?;
        latex_to_svg(text.trim(), font, &SvgOptions::default())
            .map_err(|error| EmbedError::Failed(explain(&format!("{error}"))))
    }
}

impl RenderEmbed for MathRenderer {
    fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
        super::diagram::rasterize(&self.svg(&source.text)?)
    }
}

/// 誤りの文言を、利用者が次に何をすればよいか分かる形にする。
///
/// **「missing glyph for '面'」だけでは何をすればよいか分からない。**
/// 数式フォント（STIX Two Math）に CJK が無いことが原因で、
/// latex-rust には別フォントへ退避する口が無い（OPEN-211）。
fn explain(reason: &str) -> String {
    if reason.contains("missing glyph") {
        format!("{reason}（数式の中では日本語を使えません。数式の外に書いてください）")
    } else {
        reason.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EmbedKind;

    /// 暗い画素を数える（§7.3 と同じ指標）。
    ///
    /// **透明でない画素を数えてはいけない。** 背景を塗る SVG では
    /// 常に「幅 × 高さ」になり、何も検出できない（図で実際に踏んだ）。
    fn dark_pixels(embed: &RenderedEmbed) -> usize {
        embed
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 0 && (px[0] as u16 + px[1] as u16 + px[2] as u16) < 384)
            .count()
    }

    fn render(latex: &str) -> Result<RenderedEmbed, EmbedError> {
        MathRenderer::new().render(&EmbedSource::new(EmbedKind::Math, latex, 800.0))
    }

    #[test]
    fn simple_formula_is_drawn() {
        let embed = render("E = mc^2").expect("描画できる");
        assert!(embed.width > 0 && embed.height > 0);
        assert!(
            dark_pixels(&embed) > 50,
            "中身が空: {}",
            dark_pixels(&embed)
        );
    }

    #[test]
    fn fraction_and_integral_are_drawn() {
        let embed = render(r"\int_0^\infty e^{-x^2}dx = \frac{\sqrt{\pi}}{2}").expect("描画できる");
        assert!(dark_pixels(&embed) > 100);
    }

    /// **出力はすべて `<path>`。** 外部フォントが要らないことの担保である。
    #[test]
    fn output_has_no_text_elements() {
        let font = math_font().expect("フォントを読める");
        let svg = latex_to_svg("E = mc^2", font, &SvgOptions::default()).expect("SVG を作れる");
        assert_eq!(svg.matches("<text").count(), 0, "<text> が混ざっている");
        assert!(svg.matches("<path").count() > 0);
    }

    /// 複数行の数式が積まれること。
    #[test]
    fn aligned_stacks_rows() {
        let one = render("a = b").expect("描画できる");
        let two = render(r"\begin{aligned} a &= b \\ c &= d \end{aligned}").expect("描画できる");
        assert!(
            two.height > one.height,
            "2 行が積まれていない: {} vs {}",
            two.height,
            one.height
        );
    }

    /// **日本語を使えないことを、利用者に分かる言葉で伝える**（OPEN-211）。
    #[test]
    fn japanese_in_text_reports_what_to_do() {
        let error = render(r"\text{面積}").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("日本語"), "{message}");
        assert!(message.contains("数式の外"), "{message}");
    }

    /// **構文誤りは失敗として返す。** 黙って空を出さない。
    #[test]
    fn broken_latex_fails() {
        assert!(render(r"\frac{").is_err(), "誤った入力が成功した");
    }

    #[test]
    fn empty_input_is_not_a_panic() {
        // 成否は問わない。**落ちないこと**を確かめる
        let _ = render("");
    }
}

/// OPEN-211 / OPEN-212 の再測定（§7.4）。
///
/// どちらも技術選定の時点で未解決のまま残った。実装時に測り直す。
#[cfg(test)]
mod open_items {
    use super::*;

    #[test]
    #[ignore]
    fn report() {
        let font = match math_font() {
            Ok(font) => font,
            Err(reason) => {
                println!("フォントを読めない: {reason}");
                return;
            }
        };
        let cases = [
            ("OPEN-211 日本語", r"\text{面積}"),
            ("OPEN-211 英字", r"\text{area}"),
            ("OPEN-212 2行1列", r"\begin{pmatrix} a \\ b \end{pmatrix}"),
            (
                "OPEN-212 2行2列",
                r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            ),
            (
                "参考 aligned",
                r"\begin{aligned} a &= b \\ c &= d \end{aligned}",
            ),
            ("参考 1行", "a = b"),
        ];
        // 同梱の日本語フォントを数式フォントとして使えるか
        // （MathFont::from_bytes はフォントを**差し替える**もので、併用の口は無い）
        match MathFont::from_bytes(crate::render::fonts::EMBEDDED[2]) {
            Ok(japanese) => match latex_to_svg("a = b", &japanese, &SvgOptions::default()) {
                Ok(_) => println!("日本語フォントを数式フォントに | 成功（数式が組める）"),
                Err(error) => println!("日本語フォントを数式フォントに | 組版に失敗 | {error}"),
            },
            Err(error) => println!("日本語フォントを数式フォントに | 読めない | {error}"),
        }

        for (label, latex) in cases {
            match latex_to_svg(latex, font, &SvgOptions::default()) {
                Ok(svg) => {
                    let size = super::super::diagram::rasterize(&svg)
                        .map(|e| (e.width, e.height))
                        .unwrap_or((0, 0));
                    println!("{label:<20} | 成功 | {}x{}", size.0, size.1);
                }
                Err(error) => println!("{label:<20} | 失敗 | {error}"),
            }
        }
    }
}
