//! HTML 出力（§17A）。
//!
//! **1 ファイルで完結させる。** CSS は `<style>` に入れ、画像は `data:` にし、
//! 図と数式は SVG のまま埋める。外部参照を持たないので、受け取った側は
//! そのファイルだけで読める。
//!
//! **画面や PDF とは一致しない。** HTML だけはブラウザがレイアウトするためで、
//! これは原理的な差である（§17A.3）。

use std::path::Path;

use comrak::nodes::{Ast, AstNode, LineColumn, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

use super::ExportWatch;
use crate::document::Document;
use crate::embed::{DiagramRenderer, EmbedError, MathRenderer};
use crate::export::pdf::ExportError;

/// 出力した HTML と、埋め込めなかった画像の件数（§17A.5）。
#[derive(Debug)]
pub struct Html {
    pub text: String,
    pub skipped_images: usize,
}

/// 画像 1 件あたりの埋め込み上限（§17A.5）。
///
/// **青天井にしない。** `data:` は元の約 1.33 倍になるため、大きな画像を
/// そのまま入れるとブラウザで開けなくなる
pub const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

/// 差し込み位置の目印。
///
/// **comrak に生の HTML を通させない**（§16.1 の「生 HTML は表示しない」）。
/// 通すと利用者が書いた HTML まで出てしまうので、いったん目印を置き、
/// HTML になったあとで置き換える
fn marker(index: usize) -> String {
    format!("@@MV-EMBED-{index}@@")
}

/// 文書を 1 ファイルの HTML にする。
pub fn export(
    document: &Document,
    title: &str,
    base_dir: Option<&Path>,
    watch: &dyn ExportWatch,
) -> Result<Html, ExportError> {
    watch.total(1);
    if watch.cancelled() {
        return Err(ExportError::Cancelled);
    }

    let source = document.text().to_string();
    // **`$$` を数式の囲みへ直してから渡す**（§16.6）。
    //
    // 画面と PDF は自前の走査器を通すので `$$` を数式として扱えるが、
    // ここは comrak が解析するため、そのままでは段落になる。
    // **判定は走査器に聞く**（決める場所を増やさない）
    let source = crate::parse::dollar::to_math_fences(&source);
    let arena = Arena::new();
    let options = options();
    let root = parse_document(&arena, &source, &options);

    // 目印と、その差し替え先
    let mut replacements: Vec<(String, String)> = Vec::new();
    let mut skipped_images = 0_usize;

    let diagrams = DiagramRenderer::new();
    let math = MathRenderer::new();

    // **先に集めてから変える。** 走査しながら木を変えると comrak が止める
    let nodes: Vec<_> = root.descendants().collect();

    for node in nodes {
        if watch.cancelled() {
            return Err(ExportError::Cancelled);
        }

        let replacement = {
            let data = node.data.borrow();
            match &data.value {
                NodeValue::CodeBlock(code) => {
                    let language = code.info.split_whitespace().next().unwrap_or("");
                    match language.to_ascii_lowercase().as_str() {
                        // **描けない図は原文のまま残す**（OPEN-210）。
                        // 画面・PDF と同じ判断をする（§10.30）
                        "mermaid" if crate::embed::diagram::is_supported(&code.literal) => {
                            Some(figure(diagrams.svg(&code.literal)))
                        }
                        "math" | "latex" | "katex" => Some(figure(math.svg(&code.literal))),
                        _ => None,
                    }
                }
                NodeValue::Image(image) => {
                    match embed_image(&image.url, base_dir) {
                        Some(data_uri) => Some(format!(
                            r#"<p><img src="{data_uri}" alt="{}"></p>"#,
                            escape(&image.title)
                        )),
                        // **埋め込めなかったものは件数で知らせる**（§17A.5）。
                        // 外部参照は持たないので、画像そのものは落とす
                        None => {
                            skipped_images += 1;
                            Some(format!(
                                r#"<p class="mv-missing">［画像を埋め込めません: {}］</p>"#,
                                escape(&image.url)
                            ))
                        }
                    }
                }
                _ => None,
            }
        };

        let Some(html) = replacement else {
            continue;
        };

        let index = replacements.len();
        replacements.push((format!("<p>{}</p>", marker(index)), html));

        // 段落 1 つに置き換える。**出来上がりの形が読めるようにする**ため
        let paragraph = arena.alloc(AstNode::new(std::cell::RefCell::new(Ast::new(
            NodeValue::Paragraph,
            LineColumn { line: 0, column: 0 },
        ))));
        let text = arena.alloc(AstNode::new(std::cell::RefCell::new(Ast::new(
            NodeValue::Text(marker(index).into()),
            LineColumn { line: 0, column: 0 },
        ))));
        paragraph.append(text);

        // 画像は段落の中にあるので、包んでいる段落ごと置き換える
        let target = if matches!(node.data.borrow().value, NodeValue::Image(_)) {
            node.parent().unwrap_or(node)
        } else {
            node
        };
        target.insert_before(paragraph);
        target.detach();
    }

    let mut body = String::new();
    format_html(root, &options, &mut body)
        .map_err(|error| ExportError::Write(format!("{error}")))?;

    for (marker, html) in replacements {
        body = body.replace(&marker, &html);
    }

    watch.done(1);
    Ok(Html {
        text: wrap(title, &body),
        skipped_images,
    })
}

/// 解析の設定。**画面と同じ**にする（GFM）。
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    // **生 HTML は表示せず、原文をそのまま出す**（§16.1）。
    // 既定は「省いた」という注記に置き換わるだけで、原文が消える
    options.render.escape = true;
    options
}

/// 図・数式の差し替え先。失敗しても出力は続ける（§17.8 と同じ扱い）。
fn figure(svg: Result<String, EmbedError>) -> String {
    match svg {
        Ok(svg) => format!(r#"<figure class="mv-embed">{svg}</figure>"#),
        Err(error) => format!(
            r#"<p class="mv-missing">［描画に失敗: {}］</p>"#,
            escape(&format!("{error}"))
        ),
    }
}

/// ローカル画像を `data:` にする。取れないものは `None`。
fn embed_image(url: &str, base_dir: Option<&Path>) -> Option<String> {
    // **外部参照は持たない**（§17A.2）。取りにも行かない（§16.12）
    if crate::embed::is_remote(url) {
        return None;
    }
    let path = crate::embed::resolve(url, base_dir);
    let size = std::fs::metadata(&path).ok()?.len();
    if size > MAX_IMAGE_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some(format!("data:{};base64,{}", mime_of(&path), base64(&bytes)))
}

/// 拡張子から MIME 型を決める。
fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Base64（RFC 4648）。**依存を増やさない**ために自前で持つ。
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for slot in 0..4 {
            if slot <= chunk.len() {
                let index = (triple >> (18 - slot * 6)) & 0b11_1111;
                out.push(TABLE[index as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// 属性へ入れる文字を安全にする。
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 本文を 1 ファイルの HTML に包む。
fn wrap(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
{CSS}
</style>
</head>
<body>
<main>
{body}</main>
</body>
</html>
"#,
        title = escape(title)
    )
}

/// 埋め込む CSS（§17A.2）。**Light 固定**にして、受け取った側の設定に依存させない。
const CSS: &str = r#"
/* **書体を明示する。** 指定しないと閲覧環境で中国語の字形が出る（§17A.2） */
body {
  font-family: "IBM Plex Sans JP", "Noto Sans JP", "Yu Gothic", "Hiragino Sans", sans-serif;
  /* 受け取った側の設定に依らないよう、明るい配色で固定する */
  color: #1b1b1b;
  background: #ffffff;
  margin: 0;
  line-height: 1.8;
}
main { max-width: 46rem; margin: 0 auto; padding: 2rem 1.25rem 6rem; }
h1, h2, h3, h4, h5, h6 { line-height: 1.4; margin: 2.2em 0 0.8em; }
h1 { font-size: 1.8rem; border-bottom: 1px solid #ddd; padding-bottom: 0.3em; }
h2 { font-size: 1.45rem; border-bottom: 1px solid #eee; padding-bottom: 0.2em; }
code, pre {
  font-family: "PlemolJP", "Consolas", "Menlo", monospace;
  font-size: 0.92em;
}
code { background: #f3f3f5; padding: 0.1em 0.3em; border-radius: 3px; }
pre { background: #f6f6f8; padding: 0.9em 1em; border-radius: 6px; overflow-x: auto; }
pre code { background: none; padding: 0; }
blockquote {
  margin: 1.2em 0; padding: 0.2em 1em;
  border-left: 3px solid #cfcfd4; color: #4a4a52;
}
table { border-collapse: collapse; margin: 1.2em 0; }
th, td { border: 1px solid #d8d8de; padding: 0.4em 0.8em; }
th { background: #f3f3f5; }
img, svg { max-width: 100%; height: auto; }
figure.mv-embed { margin: 1.4em 0; text-align: center; }
/* 埋め込めなかったものは、黙って消さずに跡を残す */
.mv-missing { color: #8a5b00; background: #fff6e0; padding: 0.4em 0.8em; border-radius: 4px; }
ul.contains-task-list { list-style: none; padding-left: 1.2em; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::Silent;

    fn html(text: &str) -> String {
        export(
            &Document::from_text(text.to_owned()),
            "題",
            Some(Path::new("samples")),
            &Silent,
        )
        .expect("出力できる")
        .text
    }

    #[test]
    fn it_is_a_whole_html_file() {
        let out = html("# 見出し\n\n本文です。\n");
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains(r#"<meta charset="utf-8">"#));
        assert!(out.contains("<h1>見出し</h1>"));
        assert!(out.contains("<p>本文です。</p>"));
    }

    /// **外部参照を持たない**（§17A.2）。
    #[test]
    fn nothing_is_fetched_from_outside() {
        let out = html("# 題\n\n本文\n");
        for pattern in ["http://", "https://", "<link", "<script"] {
            assert!(!out.contains(pattern), "{pattern} が入っている");
        }
    }

    /// **書体を明示する**（§17A.2）。指定しないと中国語の字形が出る
    #[test]
    fn the_font_is_named() {
        let out = html("本文\n");
        assert!(out.contains("IBM Plex Sans JP"));
        assert!(out.contains("PlemolJP"));
    }

    /// GFM の表とチェックリストが出る。
    #[test]
    fn gfm_tables_and_task_lists_are_rendered() {
        let out = html("| 見出し |\n| --- |\n| 値 |\n\n- [x] 済んだ\n");
        assert!(out.contains("<table>"));
        assert!(out.contains("checkbox"));
    }

    /// **生 HTML は出さない**（§16.1）。
    #[test]
    fn raw_html_is_escaped() {
        let out = html("<b>太字にしたい</b>\n\n<script>alert(1)</script>\n");
        assert!(!out.contains("<b>太字にしたい</b>"));
        assert!(!out.contains("<script>alert(1)</script>"));
        assert!(out.contains("&lt;b&gt;"));
    }

    /// ローカル画像は `data:` になる（§17A.2）。
    #[test]
    fn local_images_are_inlined() {
        let out = html("![見本](img/sample.png)\n");
        assert!(out.contains("data:image/png;base64,"), "埋め込まれていない");
        assert!(!out.contains("img/sample.png"), "元のパスが残っている");
    }

    /// **外部の画像は埋め込まず、跡を残す**（§17A.5）。
    #[test]
    fn remote_images_are_reported() {
        let result = export(
            &Document::from_text("![遠い](https://example.com/a.png)\n".to_owned()),
            "題",
            None,
            &Silent,
        )
        .expect("出力できる");

        assert_eq!(result.skipped_images, 1);
        assert!(result.text.contains("画像を埋め込めません"));
        assert!(!result.text.contains("<img"), "外部を参照している");
    }

    /// 無い画像でも出力は続く。
    #[test]
    fn a_missing_image_does_not_stop_the_export() {
        let result = export(
            &Document::from_text("![無い](img/none.png)\n\n本文\n".to_owned()),
            "題",
            Some(Path::new("samples")),
            &Silent,
        )
        .expect("出力できる");

        assert_eq!(result.skipped_images, 1);
        assert!(result.text.contains("<p>本文</p>"));
    }

    /// 図は SVG のまま入る（§17A.2）。**画素にしない**
    #[test]
    fn diagrams_are_embedded_as_svg() {
        let out = html("```mermaid\ngraph TD\n  A --> B\n```\n");
        assert!(out.contains("<svg"), "SVG が入っていない");
        assert!(out.contains("mv-embed"));
        assert!(!out.contains("language-mermaid"), "記法のまま残っている");
    }

    /// **erDiagram は図にせず原文を出す**（OPEN-210。利用者の報告。§10.30）。
    ///
    /// 画面・PDF と同じ判断をする。HTML だけ別に判断していたため、
    /// ラベルの欠けた図が出ていた
    #[test]
    fn er_diagrams_stay_as_source() {
        let out = html(
            "```mermaid
erDiagram
  CUSTOMER ||--o{ ORDER : places
```
",
        );
        assert!(out.contains("erDiagram"), "原文が消えている");
        assert!(out.contains("CUSTOMER"), "原文が消えている");
        assert!(!out.contains("<svg"), "図にしてしまっている");
    }

    /// 数式も SVG のまま入る。
    #[test]
    fn math_is_embedded_as_svg() {
        let out = html("```math\nE = mc^2\n```\n");
        assert!(out.contains("<svg"), "SVG が入っていない");
    }

    /// 描けなくても出力は続く（§17.8 と同じ扱い）。
    #[test]
    fn a_broken_diagram_leaves_a_note() {
        let out = html("```mermaid\nこれは図ではない\n```\n");
        assert!(out.contains("描画に失敗"), "{out}");
    }

    /// 取り消せる（§17.10）。
    #[test]
    fn it_can_be_cancelled() {
        struct Stop;
        impl ExportWatch for Stop {
            fn cancelled(&self) -> bool {
                true
            }
        }
        let error = export(&Document::from_text("本文\n".to_owned()), "題", None, &Stop)
            .expect_err("取り消したのに出力された");
        assert!(matches!(error, ExportError::Cancelled));
    }

    /// Base64 は RFC 4648 のとおり（詰めも含めて）。
    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }
}
