//! 可視範囲のインライン解析（§12.7）。
//!
//! **ブロック単位で comrak にかける。** 設計は「可視範囲 64KB をまとめて」と
//! 書いているが、ブロック単位のほうがレイアウトキャッシュ（§3.8）の粒度と
//! 一致し、1 ブロックだけ編集されたときに他を作り直さずに済む。
//!
//! ブロックは空行で区切られているため、単体でも Markdown の断片として成立する。
//!
//! **comrak の型を層の外へ出さない**（§6.2）。ここで自前の構造へ写し替える。

use comrak::nodes::{AstNode, ListDelimType, ListType, NodeValue, TableAlignment};
use comrak::{parse_document, Arena, Options};

use super::BlockKind;

/// インライン要素の見た目。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanStyle {
    #[default]
    Normal,
    /// 斜体（`*text*`）
    Emphasis,
    /// 太字（`**text**`）
    Strong,
    /// インラインコード（`` `code` ``）
    Code,
    /// リンク。本文だけを出し、URL は出さない
    Link,
}

/// 同じ見た目が続く範囲。
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

impl Span {
    fn new(text: impl Into<String>, style: SpanStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

/// 折り返し前の 1 行（段落なら 1 行、リストなら 1 項目）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LogicalLine {
    pub spans: Vec<Span>,
    /// 字下げの段数（リストの入れ子・引用）
    pub indent: u8,
    /// リストの行頭記号（`• ` や `1. `）。無ければ `None`
    pub marker: Option<String>,
}

impl LogicalLine {
    fn is_empty(&self) -> bool {
        self.marker.is_none() && self.spans.iter().all(|span| span.text.trim().is_empty())
    }
}

/// ブロックの中身。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlockContent {
    pub lines: Vec<LogicalLine>,
    /// **段落全体が画像 1 つ**だったときの参照先。
    ///
    /// レイアウト層はこれを見て、段落ではなく画像の箱を置く（§16.12）。
    /// 文章の途中にある画像はここに入らない。
    pub lone_image: Option<String>,
}

/// セルの寄せ方（区切り行の `:` で決まる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 表の 1 セル。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableCell {
    pub spans: Vec<Span>,
}

/// 表の 1 行。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    /// 見出し行か
    pub header: bool,
}

/// 表の中身。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableContent {
    pub rows: Vec<TableRow>,
    pub aligns: Vec<CellAlign>,
}

impl TableContent {
    /// 列数。**行によってセル数が違いうる**ので最大を取る。
    pub fn columns(&self) -> usize {
        self.rows
            .iter()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(0)
    }

    pub fn align(&self, column: usize) -> CellAlign {
        self.aligns.get(column).copied().unwrap_or_default()
    }
}

/// 表の原文を行と列へ分解する。
pub fn parse_table(source: &str) -> TableContent {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());

    let mut content = TableContent::default();
    collect_table(root, &mut content);
    content
}

fn collect_table<'a>(node: &'a AstNode<'a>, out: &mut TableContent) {
    for child in node.children() {
        let value = child.data.borrow();
        match &value.value {
            NodeValue::Table(table) => {
                out.aligns = table
                    .alignments
                    .iter()
                    .map(|align| match align {
                        TableAlignment::Center => CellAlign::Center,
                        TableAlignment::Right => CellAlign::Right,
                        _ => CellAlign::Left,
                    })
                    .collect();
                drop(value);
                for row in child.children() {
                    let header = matches!(row.data.borrow().value, NodeValue::TableRow(true));
                    let cells = row
                        .children()
                        .map(|cell| {
                            let mut spans = Vec::new();
                            collect_spans(cell, SpanStyle::Normal, &mut spans);
                            TableCell { spans }
                        })
                        .collect();
                    out.rows.push(TableRow { cells, header });
                }
                return;
            }
            _ => {
                drop(value);
                collect_table(child, out);
            }
        }
    }
}

/// 入れ子の上限（§3.4）。異常な文書でレイアウトが破綻しないようにする。
const MAX_DEPTH: u8 = 8;

/// ブロックの原文をインライン要素へ分解する。
///
/// コードブロックと表は対象外（原文の行をそのまま使う。§3.5）。
pub fn parse_block(source: &str, kind: &BlockKind) -> BlockContent {
    if matches!(kind, BlockKind::Code { .. } | BlockKind::Table) {
        return BlockContent::default();
    }

    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());

    let mut content = BlockContent {
        lone_image: lone_image(root),
        ..Default::default()
    };
    collect(root, 0, &mut content);
    content.lines.retain(|line| !line.is_empty());

    // **ここで行を空にしてはいけない。** 箱を置くかどうかを決めるのは
    // レイアウト層である。外部の画像のように箱を置かない場合、
    // 行まで消すと画面に何も出なくなる（実際に踏んだ）。
    // 画像だけの段落で箱を置いたときは、レイアウト層が行を使わない
    content
}

/// 段落全体が画像 1 つだけなら、その参照先を返す。
///
/// **前後の空白以外に何も無いこと**を条件にする。`![a](x) と書いた` のように
/// 文章が続く場合は、行の中に箱を置く必要があり、ここでは扱わない。
fn lone_image<'a>(root: &'a AstNode<'a>) -> Option<String> {
    let mut children = root.children();
    let paragraph = children.next()?;
    if children.next().is_some() {
        return None;
    }
    if !matches!(paragraph.data.borrow().value, NodeValue::Paragraph) {
        return None;
    }

    let mut url = None;
    for child in paragraph.children() {
        match &child.data.borrow().value {
            NodeValue::Image(link) => {
                if url.is_some() {
                    return None;
                }
                url = Some(link.url.clone());
            }
            // 空白だけなら無視する
            NodeValue::Text(text) if text.trim().is_empty() => {}
            NodeValue::SoftBreak => {}
            _ => return None,
        }
    }
    url
}

/// comrak の設定。段落でも表でも同じものを使う。
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options
}

/// 木をたどって論理行を集める。
fn collect<'a>(node: &'a AstNode<'a>, depth: u8, out: &mut BlockContent) {
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Paragraph | NodeValue::Heading(_) => {
                let mut line = LogicalLine {
                    indent: depth.min(MAX_DEPTH),
                    ..Default::default()
                };
                collect_spans(child, SpanStyle::Normal, &mut line.spans);
                out.lines.push(line);
            }

            NodeValue::List(list) => {
                let ordered = list.list_type == ListType::Ordered;
                let mut number = list.start.max(1);
                for item in child.children() {
                    // **チェックリストは箱を出す**（受入条件 §23.1）。
                    // `- [ ]` と `- [x]` は行頭記号そのものが違う
                    let marker = match item.data.borrow().value {
                        NodeValue::TaskItem(task) if task.symbol.is_some() => "☑ ".to_owned(),
                        NodeValue::TaskItem(_) => "☐ ".to_owned(),
                        _ if ordered => {
                            let delimiter = if list.delimiter == ListDelimType::Paren {
                                ')'
                            } else {
                                '.'
                            };
                            let text = format!("{number}{delimiter} ");
                            number += 1;
                            text
                        }
                        _ => "• ".to_owned(),
                    };
                    collect_item(item, depth, Some(marker), out);
                }
            }

            NodeValue::BlockQuote => collect(child, depth.saturating_add(1), out),

            NodeValue::ThematicBreak => out.lines.push(LogicalLine {
                spans: vec![Span::new("─".repeat(16), SpanStyle::Normal)],
                indent: depth.min(MAX_DEPTH),
                marker: None,
            }),

            // コードブロックと表はここでは扱わない（§3.5）
            NodeValue::CodeBlock(_) | NodeValue::Table(_) => {}

            _ => collect(child, depth, out),
        }
    }
}

/// リスト項目を 1 行として取り込む。
fn collect_item<'a>(
    item: &'a AstNode<'a>,
    depth: u8,
    marker: Option<String>,
    out: &mut BlockContent,
) {
    let mut first = true;
    for child in item.children() {
        match &child.data.borrow().value {
            NodeValue::Paragraph => {
                let mut line = LogicalLine {
                    indent: depth.min(MAX_DEPTH),
                    marker: if first { marker.clone() } else { None },
                    ..Default::default()
                };
                collect_spans(child, SpanStyle::Normal, &mut line.spans);
                out.lines.push(line);
                first = false;
            }
            // 入れ子のリストは 1 段深くする
            NodeValue::List(_) => collect(item, depth.saturating_add(1), out),
            _ => {}
        }
    }
}

/// インライン要素を平らな範囲の列にする。
fn collect_spans<'a>(node: &'a AstNode<'a>, inherited: SpanStyle, out: &mut Vec<Span>) {
    for child in node.children() {
        let value = child.data.borrow();
        match &value.value {
            NodeValue::Text(text) => push(out, text, inherited),
            NodeValue::Code(code) => push(out, &code.literal, SpanStyle::Code),

            // 空白 1 つに畳む。折り返しは幅で決まるので原文の改行は引き継がない
            NodeValue::SoftBreak | NodeValue::LineBreak => push(out, " ", inherited),

            NodeValue::Emph => {
                drop(value);
                collect_spans(child, SpanStyle::Emphasis, out);
                continue;
            }
            NodeValue::Strong => {
                drop(value);
                collect_spans(child, SpanStyle::Strong, out);
                continue;
            }
            // リンクは**本文だけ**を出す。URL は出さない
            NodeValue::Link(link) => {
                let url = link.url.clone();
                drop(value);
                let mut inner = Vec::new();
                collect_spans(child, SpanStyle::Link, &mut inner);
                emit_link(out, &url, inner, inherited);
                continue;
            }
            // **画像は文字ではない。** 文中の画像はここで目印に置き換える。
            //
            // 段落全体が画像 1 つのときはレイアウト層が箱に置き換えるので、
            // ここで作った目印は使われない（`BlockContent::lone_image`）。
            NodeValue::Image(link) => {
                let label = image_label(link.url.as_str(), child);
                push(out, &label, inherited);
            }
            NodeValue::Strikethrough => {
                drop(value);
                collect_spans(child, inherited, out);
                continue;
            }
            // 生 HTML は表示しない（§3.3）。原文をそのまま出す
            NodeValue::HtmlInline(html) => push(out, html, SpanStyle::Code),

            _ => {
                drop(value);
                collect_spans(child, inherited, out);
                continue;
            }
        }
    }
}

/// リンクを出す。**裸の URL は非 ASCII 文字で打ち切る。**
///
/// comrak の autolink は URL の終端を ASCII の区切りで判定するため、
/// `URL（https://example.com/a）が…` のように後ろが日本語だと、
/// **全角文字を URL の一部とみなして行末まで飲み込む**（実際に踏んだ）。
///
/// URL は RFC 3986 で ASCII のみと決まっている。最初の非 ASCII 文字で切り、
/// 残りは地の文として出す。`[本文](url)` の形はこの対象外である
/// （本文と URL が一致しないため見分けられる）。
fn emit_link(out: &mut Vec<Span>, url: &str, inner: Vec<Span>, inherited: SpanStyle) {
    let text: String = inner.iter().map(|span| span.text.as_str()).collect();

    // 本文と URL が同じ = 裸の URL（autolink）
    if text == url {
        if let Some(cut) = text.find(|ch: char| !ch.is_ascii()) {
            push(out, &text[..cut], SpanStyle::Link);
            push(out, &text[cut..], inherited);
            return;
        }
    }

    for span in inner {
        push(out, &span.text, span.style);
    }
}

/// 画像の目印。**何の画像かが分かる文言にする。**
///
/// `［画像］` だけでは、読み手は何が入るはずだったのか分からない。
/// 取得しない外部画像は、**取得しないことを明示する**（§16.12）。
fn image_label<'a>(url: &str, node: &'a AstNode<'a>) -> String {
    let mut alt = String::new();
    for child in node.children() {
        if let NodeValue::Text(text) = &child.data.borrow().value {
            alt.push_str(text);
        }
    }
    let alt = alt.trim();

    if crate::embed::is_remote(url) {
        return format!("［外部の画像は取得しません: {url}］");
    }
    if alt.is_empty() {
        "［画像］".to_owned()
    } else {
        format!("［画像: {alt}］")
    }
}

/// 同じ見た目が続くなら 1 つにまとめる。描画の回数を減らす。
fn push(out: &mut Vec<Span>, text: &str, style: SpanStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.style == style {
            last.text.push_str(text);
            return;
        }
    }
    out.push(Span::new(text, style));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> BlockContent {
        parse_block(source, &BlockKind::Paragraph)
    }

    fn flat(content: &BlockContent) -> String {
        content
            .lines
            .iter()
            .map(|line| {
                let marker = line.marker.clone().unwrap_or_default();
                let body: String = line.spans.iter().map(|s| s.text.as_str()).collect();
                format!("{marker}{body}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn plain_paragraph() {
        let content = parse("これは段落です。\n");
        assert_eq!(flat(&content), "これは段落です。");
        assert_eq!(content.lines[0].spans[0].style, SpanStyle::Normal);
    }

    /// **記法そのものは表示されない。** これがプレビューの要点である。
    #[test]
    fn emphasis_markers_are_removed() {
        let content = parse("これは **太字** と *斜体* です。\n");
        assert_eq!(flat(&content), "これは 太字 と 斜体 です。");

        let styles: Vec<_> = content.lines[0].spans.iter().map(|s| s.style).collect();
        assert!(styles.contains(&SpanStyle::Strong));
        assert!(styles.contains(&SpanStyle::Emphasis));
    }

    #[test]
    fn inline_code_keeps_content_only() {
        let content = parse("設定は `config.toml` にある。\n");
        assert_eq!(flat(&content), "設定は config.toml にある。");
        let code = content.lines[0]
            .spans
            .iter()
            .find(|s| s.style == SpanStyle::Code)
            .expect("コードの範囲がある");
        assert_eq!(code.text, "config.toml");
    }

    /// リンクは**本文だけ**を出す。URL は出さない。
    #[test]
    fn link_shows_text_not_url() {
        let content = parse("詳しくは [設計書](design/foo.md) を参照。\n");
        assert_eq!(flat(&content), "詳しくは 設計書 を参照。");
        assert!(content.lines[0]
            .spans
            .iter()
            .any(|s| s.style == SpanStyle::Link));
    }

    /// **裸の URL は非 ASCII 文字で打ち切る。**
    ///
    /// comrak の autolink は全角文字を URL の一部とみなし、
    /// 日本語文書では行末まで飲み込む（実際に踏んだ）。
    #[test]
    fn bare_url_stops_at_japanese_text() {
        let content = parse(
            "URL（https://example.com/a/b）が幅を超える。
",
        );
        let spans = &content.lines[0].spans;

        let link = spans
            .iter()
            .find(|span| span.style == SpanStyle::Link)
            .expect("リンクがある");
        assert_eq!(link.text, "https://example.com/a/b");

        // 後ろの日本語は地の文に戻る
        let tail: String = spans
            .iter()
            .filter(|span| span.style != SpanStyle::Link)
            .map(|span| span.text.as_str())
            .collect();
        assert!(tail.contains("が幅を超える。"), "{tail}");
        // 表示される文字は原文どおり
        assert_eq!(
            flat(&content),
            "URL（https://example.com/a/b）が幅を超える。"
        );
    }

    /// ASCII だけで終わる裸の URL は、そのままリンクのまま。
    #[test]
    fn ascii_only_bare_url_is_kept() {
        let content = parse(
            "see https://example.com/a/b for details
",
        );
        let link = content.lines[0]
            .spans
            .iter()
            .find(|span| span.style == SpanStyle::Link)
            .expect("リンクがある");
        assert_eq!(link.text, "https://example.com/a/b");
    }

    /// `[本文](url)` は打ち切りの対象外（本文と URL が一致しないため）。
    #[test]
    fn explicit_link_with_japanese_label_is_untouched() {
        let content = parse(
            "詳しくは [設計書の第 1 章](design/a.md) を見よ。
",
        );
        let link = content.lines[0]
            .spans
            .iter()
            .find(|span| span.style == SpanStyle::Link)
            .expect("リンクがある");
        assert_eq!(link.text, "設計書の第 1 章");
    }

    #[test]
    fn heading_text_without_hashes() {
        let content = parse_block("## 小見出し\n", &BlockKind::Heading(2));
        assert_eq!(flat(&content), "小見出し");
    }

    #[test]
    fn bullet_list_gets_markers() {
        let content = parse_block("- 項目 A\n- 項目 B\n", &BlockKind::List);
        assert_eq!(flat(&content), "• 項目 A\n• 項目 B");
    }

    /// **チェックリストは箱で出す**（受入条件 §23.1）。
    #[test]
    fn task_list_shows_checkboxes() {
        let content = parse_block(
            "- [ ] まだ\n- [x] 済んだ\n- [X] 大文字も済み\n",
            &BlockKind::List,
        );
        assert_eq!(flat(&content), "☐ まだ\n☑ 済んだ\n☑ 大文字も済み");
    }

    /// 記法の `[ ]` が本文に残らない。
    #[test]
    fn task_list_markup_is_removed() {
        let content = parse_block("- [x] 済んだ\n", &BlockKind::List);
        let body: String = content.lines[0]
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(body.trim(), "済んだ");
    }

    /// ふつうのリストは箱にしない。
    #[test]
    fn plain_list_keeps_its_bullet() {
        let content = parse_block("- ふつうの項目\n", &BlockKind::List);
        assert_eq!(flat(&content), "• ふつうの項目");
    }

    #[test]
    fn ordered_list_numbers_increase() {
        let content = parse_block("1. 一つ目\n2. 二つ目\n3. 三つ目\n", &BlockKind::List);
        assert_eq!(flat(&content), "1. 一つ目\n2. 二つ目\n3. 三つ目");
    }

    #[test]
    fn quote_is_indented() {
        let content = parse_block("> 引用です\n", &BlockKind::Quote);
        assert_eq!(flat(&content), "引用です");
        assert_eq!(content.lines[0].indent, 1);
    }

    /// 原文の改行は空白 1 つに畳む。折り返しは幅で決まる。
    #[test]
    fn soft_breaks_become_spaces() {
        let content = parse("一行目の続きが\n二行目にある。\n");
        assert_eq!(content.lines.len(), 1);
        assert_eq!(flat(&content), "一行目の続きが 二行目にある。");
    }

    const TABLE: &str = "| 名前 | 値 | 備考 |
|:---|---:|:---:|
| あ | 1 | **太字** |
| い | 22 | |
";

    #[test]
    fn table_rows_and_columns() {
        let table = parse_table(TABLE);
        assert_eq!(table.rows.len(), 3, "見出し 1 + 本体 2");
        assert_eq!(table.columns(), 3);
        assert!(table.rows[0].header);
        assert!(!table.rows[1].header);
    }

    #[test]
    fn table_alignment_comes_from_delimiter_row() {
        let table = parse_table(TABLE);
        assert_eq!(table.align(0), CellAlign::Left);
        assert_eq!(table.align(1), CellAlign::Right);
        assert_eq!(table.align(2), CellAlign::Center);
    }

    /// セルの中でもインライン要素は効く。
    #[test]
    fn table_cells_keep_inline_styles() {
        let table = parse_table(TABLE);
        let cell = &table.rows[1].cells[2];
        assert_eq!(cell.spans[0].text, "太字");
        assert_eq!(cell.spans[0].style, SpanStyle::Strong);
    }

    #[test]
    fn empty_cell_is_kept() {
        let table = parse_table(TABLE);
        // 列を落とすと以降の列がずれるため、空でもセルは残す
        assert_eq!(table.rows[2].cells.len(), 3);
        assert!(table.rows[2].cells[2].spans.is_empty());
    }

    #[test]
    fn non_table_source_yields_nothing() {
        assert!(parse_table(
            "ただの段落です。
"
        )
        .rows
        .is_empty());
    }

    /// コードブロックと表は `parse_block` では扱わない（別の入口を使う）。
    #[test]
    fn code_and_table_are_left_alone() {
        let code = parse_block(
            "```rust\nfn main() {}\n```\n",
            &BlockKind::Code { language: None },
        );
        assert!(code.lines.is_empty());
        let table = parse_block("| a | b |\n|---|---|\n", &BlockKind::Table);
        assert!(table.lines.is_empty());
    }

    /// **画像 1 つだけの段落**は、参照先が取れる。
    #[test]
    fn lone_image_is_detected() {
        let content = parse("![図](img/a.png)\n");
        assert_eq!(content.lone_image.as_deref(), Some("img/a.png"));
        // **行は残す。** 箱を置くかはレイアウト層が決める
        assert!(!content.lines.is_empty());
    }

    /// **外部の画像は、取得しないことを画面に出す。**
    ///
    /// 黙って空白にすると、利用者は画像が壊れているのか設定なのか分からない。
    #[test]
    fn remote_image_says_it_is_not_fetched() {
        let content = parse("![外部](https://example.com/a.png)\n");
        let text = flat(&content);
        assert!(text.contains("取得しません"), "{text}");
        assert!(text.contains("https://example.com/a.png"), "{text}");
    }

    /// 文中の画像は、**何の画像か**が分かる目印にする。
    #[test]
    fn inline_image_keeps_its_alt_text() {
        let content = parse("これは ![小さな図](a.png) の例。\n");
        let text = flat(&content);
        assert!(text.contains("［画像: 小さな図］"), "{text}");
    }

    #[test]
    fn image_without_alt_is_still_marked() {
        let content = parse("これは ![](a.png) の例。\n");
        assert!(flat(&content).contains("［画像］"));
    }

    #[test]
    fn lone_image_ignores_surrounding_space() {
        let content = parse("  ![図](a.png)  \n");
        assert_eq!(content.lone_image.as_deref(), Some("a.png"));
    }

    /// 文章が続く場合は段落として扱う（行の中に箱を置く話になるため）。
    #[test]
    fn image_with_text_is_not_lone() {
        let content = parse("![図](a.png) と書いた。\n");
        assert!(content.lone_image.is_none());
        assert!(flat(&content).contains("と書いた"));
    }

    #[test]
    fn two_images_are_not_lone() {
        let content = parse("![a](a.png)![b](b.png)\n");
        assert!(content.lone_image.is_none());
    }

    #[test]
    fn plain_paragraph_has_no_image() {
        assert!(parse("ふつうの段落\n").lone_image.is_none());
    }

    #[test]
    fn empty_source_is_safe() {
        assert!(parse("").lines.is_empty());
        assert!(parse("\n\n").lines.is_empty());
    }

    /// 同じ見た目が続く範囲はまとめる（描画回数を減らすため）。
    #[test]
    fn adjacent_spans_are_merged() {
        let content = parse("ふつうの文字が続く。\n");
        assert_eq!(content.lines[0].spans.len(), 1);
    }
}
