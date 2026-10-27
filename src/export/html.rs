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
/// 本文をコードの囲みに入れる（v2.2.0）。**囲みは本文の中のどの ` の並びより長くする**
fn fence_plain(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    let newline = if text.ends_with('\n') { "" } else { "\n" };
    format!("{fence}\n{text}{newline}{fence}\n")
}

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

    let mut source = document.text().to_string();
    // **テキストとして読む文書は、まるごとコードの囲みに入れる**（v2.2.0）。
    // comrak へそのまま渡すと `#` や `*` を Markdown として読んでしまう
    if document.is_plain() {
        source = fence_plain(&source);
    }
    // **`$$` を数式の囲みへ直してから渡す**（§16.6）。
    //
    // 画面と PDF は自前の走査器を通すので `$$` を数式として扱えるが、
    // ここは comrak が解析するため、そのままでは段落になる。
    // **判定は走査器に聞く**（決める場所を増やさない）
    let mut skipped_images = 0_usize;
    let body = body_html(&source, base_dir, watch, &mut skipped_images)?;

    watch.done(1);
    Ok(Html {
        text: wrap(title, &body),
        skipped_images,
    })
}

/// Markdown を本文の HTML にする（図・数式・画像は埋め込む）。**スライドの書き出しでも使う**（v2.4.0 R-07 7d）
pub(super) fn body_html(
    source: &str,
    base_dir: Option<&Path>,
    watch: &dyn ExportWatch,
    skipped: &mut usize,
) -> Result<String, ExportError> {
    let source = crate::parse::dollar::to_math_fences(source);
    let arena = Arena::new();
    let options = options();
    let root = parse_document(&arena, &source, &options);

    // 目印と、その差し替え先
    let mut replacements: Vec<(String, String)> = Vec::new();

    let diagrams = DiagramRenderer::new();
    let math = MathRenderer::new();

    // 段落の中の決まったタグ（v2.9.0 R-03）。**読んだタグだけを書き直す**（属性は出さない）。
    // 段落の中に置くので、段落では包まない
    if crate::parse::html_inline::enabled() {
        // **書き換える前に選ぶ**（書き換えると、残りのタグの開きと閉じがそろわなくなる）
        let nodes: Vec<_> = root
            .descendants()
            .filter_map(|node| {
                let tag = match &node.data.borrow().value {
                    NodeValue::HtmlInline(html) => crate::parse::html_inline::inline_tag(html),
                    _ => None,
                }?;
                node.parent()
                    .is_some_and(crate::parse::inline::balanced)
                    .then_some((node, tag))
            })
            .collect();
        for (node, tag) in nodes {
            let html = match (tag.name, tag.closing) {
                ("br", _) => "<br>".to_owned(),
                (name, true) => format!("</{name}>"),
                (name, false) => format!("<{name}>"),
            };
            let index = replacements.len();
            replacements.push((marker(index), html));
            let text = arena.alloc(AstNode::new(std::cell::RefCell::new(Ast::new(
                NodeValue::Text(marker(index).into()),
                LineColumn { line: 0, column: 0 },
            ))));
            node.insert_before(text);
            node.detach();
        }
    }

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
                NodeValue::Image(image) => Some(image_html(
                    &image.url,
                    &image.title,
                    None,
                    base_dir,
                    skipped,
                )),
                // **`<img>` タグ 1 つだけのものも画像として埋め込む**（v2.1.0 R-17）。
                // 生の HTML は出さない設定なので、扱わないと黙って消える
                NodeValue::HtmlBlock(html) => crate::parse::inline::img_tag(&html.literal)
                    .map(|tag| image_html(&tag.src, &tag.alt, tag.width, base_dir, skipped))
                    // HTML の表（v2.8.0 R-05）。**読んだ中身から書き直す**（元の属性は出さない）
                    .or_else(|| {
                        crate::layout::html_table_of(&html.literal).map(|table| table_html(&table))
                    })
                    // `<details>`（v2.9.0 R-03）。見出しと開き方だけを書き直す
                    .or_else(|| details_html(&html.literal, &options)),
                NodeValue::HtmlInline(html) if alone_in_paragraph(node) => {
                    crate::parse::inline::img_tag(html)
                        .map(|tag| image_html(&tag.src, &tag.alt, tag.width, base_dir, skipped))
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
        let target = if matches!(
            node.data.borrow().value,
            NodeValue::Image(_) | NodeValue::HtmlInline(_)
        ) {
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
    Ok(body)
}

/// 画像 1 つを HTML にする（埋め込めなければ、その旨の段落）。
fn image_html(
    url: &str,
    alt: &str,
    width: Option<f32>,
    base_dir: Option<&Path>,
    skipped: &mut usize,
) -> String {
    match embed_image(url, base_dir) {
        Some(data_uri) => {
            let width = width
                .map(|w| format!(r#" width="{}""#, w.round() as u32))
                .unwrap_or_default();
            format!(
                r#"<p><img src="{data_uri}" alt="{}"{width}></p>"#,
                escape(alt)
            )
        }
        // **埋め込めなかったものは件数で知らせる**（§17A.5）。
        // 外部参照は持たないので、画像そのものは落とす
        None => {
            *skipped += 1;
            format!(
                r#"<p class="mv-missing">［画像を埋め込めません: {}］</p>"#,
                escape(url)
            )
        }
    }
}

/// 段落の中身がこれ 1 つだけか（前後の空白は除く）。
fn alone_in_paragraph<'a>(node: &'a AstNode<'a>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !matches!(parent.data.borrow().value, NodeValue::Paragraph) {
        return false;
    }
    parent.children().all(|child| {
        std::ptr::eq(child, node)
            || matches!(&child.data.borrow().value,
                NodeValue::Text(text) if text.trim().is_empty())
            || matches!(child.data.borrow().value, NodeValue::SoftBreak)
    })
}

/// `<details>` の頭と終わりを書き直す（v2.9.0 R-03）。見出しは読んだ字の飾りだけ、同じブロックの中身は Markdown として
fn details_html(literal: &str, options: &Options) -> Option<String> {
    use crate::parse::html_inline;
    if !html_inline::enabled() {
        return None;
    }
    if html_inline::is_details_end(literal) {
        return Some("</details>".to_owned());
    }
    let head = html_inline::details_head(literal)?;
    let mut out = String::from(if head.open {
        "<details open>"
    } else {
        "<details>"
    });
    out.push_str("<summary>");
    if !head.summary.is_empty() {
        let content =
            crate::parse::inline::parse_block(&head.summary, &crate::parse::BlockKind::Paragraph);
        for (index, line) in content.lines.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            out.push_str(&spans_html(&line.spans));
        }
    }
    out.push_str("</summary>\n");
    if !head.rest.is_empty() {
        out.push_str(&comrak::markdown_to_html(&head.rest, options));
    }
    if head.closed {
        out.push_str("</details>");
    }
    Some(out)
}

/// 字の飾りの並びを HTML に（表のセルと `<details>` の見出し）
fn spans_html(spans: &[crate::parse::inline::Span]) -> String {
    use crate::parse::inline::SpanStyle;
    let mut out = String::new();
    for span in spans {
        let text = escape(&span.text);
        out.push_str(&match span.style {
            SpanStyle::Strong => format!("<strong>{text}</strong>"),
            SpanStyle::Emphasis => format!("<em>{text}</em>"),
            SpanStyle::Code => format!("<code>{text}</code>"),
            SpanStyle::Link => format!("<u>{text}</u>"),
            SpanStyle::Normal => text,
            style => match crate::parse::html_inline::tag_of(style) {
                Some(tag) => format!("<{tag}>{text}</{tag}>"),
                None => text,
            },
        });
    }
    out
}

/// HTML の表を書き直す（v2.8.0 R-05）。字の飾り・結合・寄せ方だけを出す（リンクの行き先は出さない）
fn table_html(table: &crate::parse::html_table::HtmlTable) -> String {
    use crate::parse::inline::CellAlign;
    let mut out = String::from("<table>\n");
    for row in &table.rows {
        out.push_str("<tr>");
        for cell in &row.cells {
            let tag = if cell.header || row.header {
                "th"
            } else {
                "td"
            };
            let mut attributes = String::new();
            if cell.colspan > 1 {
                attributes.push_str(&format!(" colspan=\"{}\"", cell.colspan));
            }
            if cell.rowspan > 1 {
                attributes.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
            }
            match cell.align {
                CellAlign::Center => attributes.push_str(" style=\"text-align: center\""),
                CellAlign::Right => attributes.push_str(" style=\"text-align: right\""),
                CellAlign::Left => {}
            }
            out.push_str(&format!("<{tag}{attributes}>"));
            for (index, line) in cell.lines.iter().enumerate() {
                if index > 0 {
                    out.push_str("<br>");
                }
                out.push_str(&spans_html(line));
            }
            out.push_str(&format!("</{tag}>"));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>");
    out
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
pub(super) fn embed_image(url: &str, base_dir: Option<&Path>) -> Option<String> {
    // 設定で入れていれば URL から取って埋める（v2.4.0 R-09。外部参照は持たないまま）
    if let Some(remote) = crate::embed::image::remote_url(url) {
        let (bytes, _) = crate::io::remote::fetch(&remote, MAX_IMAGE_BYTES).ok()?;
        let name = crate::io::remote::file_name(&remote);
        return Some(format!(
            "data:{};base64,{}",
            mime_of(Path::new(&name)),
            base64(&bytes)
        ));
    }
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
pub(super) fn base64(bytes: &[u8]) -> String {
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
pub(super) fn escape(text: &str) -> String {
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

    /// **テキストとして読む文書は、記法を解釈せずに出す**（v2.2.0）
    #[test]
    fn plain_text_is_exported_verbatim() {
        let document = Document::plain_text("# not\n```\n*x*\n".to_owned(), true);
        let out = export(&document, "題", None, &Silent)
            .expect("出力できる")
            .text;
        assert!(!out.contains("<h1>"), "見出しにした");
        assert!(!out.contains("<em>"), "強調にした");
        assert!(out.contains("# not"));
        assert_eq!(fence_plain("a"), "```\na\n```\n");
        assert_eq!(fence_plain("````\n"), "`````\n````\n`````\n");
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

    /// **`<img>` タグ 1 つだけのものも埋め込む**（v2.1.0 R-17）。幅は保ち、他の属性は捨てる
    #[test]
    fn a_lone_img_tag_is_inlined() {
        let out =
            html("<img width=\"200\" alt=\"見本\" onerror=\"alert(1)\" src=\"img/sample.png\">\n");
        assert!(out.contains("data:image/png;base64,"), "埋め込まれていない");
        assert!(out.contains(r#"width="200""#), "幅が落ちた");
        assert!(!out.contains("onerror"), "属性がそのまま出ている");
        let inline = html("前 <img src=\"img/sample.png\"> 後\n");
        assert!(!inline.contains("data:image"), "文の中のタグまで埋め込んだ");
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

    /// HTML の表は読んだ中身から書き直す（v2.8.0 R-05）。結合・寄せ方・飾りは残し、元の属性は出さない
    #[test]
    fn html_tables_are_rewritten() {
        let out = html(
            "<table>\n<tr><th onclick=\"x()\">名前</th><th align=\"right\">数</th></tr>\n<tr><td colspan=\"2\"><b>a</b> &amp; <a href=\"javascript:x\">b</a></td></tr>\n</table>\n",
        );
        assert!(out.contains("<th>名前</th>"), "{out}");
        assert!(out.contains("<th style=\"text-align: right\">数</th>"));
        assert!(
            out.contains("<td colspan=\"2\"><strong>a</strong> &amp; <u>b</u></td>"),
            "{out}"
        );
        assert!(
            !out.contains("onclick") && !out.contains("javascript"),
            "元の属性は出さない"
        );
        // 表でない HTML は、いままでどおり原文を出す
        let out = html("<div>x</div>\n");
        assert!(out.contains("&lt;div&gt;"));
    }

    /// 決まった HTML は読んだタグだけを書き直す（v2.9.0 R-03）。属性は出さない
    #[test]
    fn known_html_is_rewritten() {
        let out = html("押す <kbd onclick=\"x()\">Ctrl</kbd> と H<sub>2</sub>O<br>次\n");
        assert!(
            out.contains("押す <kbd>Ctrl</kbd> と H<sub>2</sub>O<br>"),
            "{out}"
        );
        assert!(!out.contains("onclick"));
        let out =
            html("<details open>\n<summary>見出し <kbd>A</kbd></summary>\n\n中身\n\n</details>\n");
        assert!(
            out.contains("<details open><summary>見出し <kbd>A</kbd></summary>"),
            "{out}"
        );
        assert!(
            out.contains("<p>中身</p>") && out.contains("</details>"),
            "{out}"
        );
        // 知らないタグは、いままでどおり原文を出す
        assert!(html("<span>x</span>\n").contains("&lt;span&gt;"));
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
