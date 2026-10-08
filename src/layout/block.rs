//! ブロックのレイアウト（§3.2 / §3.5 / §12.7）。
//!
//! 可視範囲に入ったブロックだけがここを通る。10MB でも 1 画面ぶん
//! （20〜60 件）しか呼ばれない（§16.6）。
//!
//! 原文をそのまま並べるのではなく、**インライン解析の結果を並べる**。
//! `**太字**` は「太字」と表示され、記法そのものは消える。

use std::ops::Range;

use unicode_linebreak::{linebreaks, BreakOpportunity};

use super::measure::{TextMeasurer, TextStyle};
use crate::embed::{EmbedKey, EmbedKind, EmbedSource, JobState, PLACEHOLDER_HEIGHT};
use crate::parse::highlight::{highlight, TokenRole};
use crate::parse::inline::{
    parse_block, parse_table, BlockContent, CellAlign, LogicalLine, Span, SpanStyle, TableContent,
};
use crate::parse::{Block, BlockKind};

/// 装飾。**どう見せるかは描画層に委ねる**ための印。
///
/// レイアウト層は「強調である」という事実だけを残し、斜体に傾けるか色を変えるかは
/// 決めない。同梱フォント（Regular / Bold）に斜体の字形は無く、合成斜体が
/// 日本語の字形に耐えるかはフォントを載せる P3 まで確かめられない（DD-OPEN-09）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunDecoration {
    #[default]
    None,
    /// 強調（`*text*`）
    Emphasis,
    /// インラインコード（`` `code` ``）
    Code,
    /// リンクの本文
    Link,
}

/// 同じ見た目が続く、行の中の一区間。
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub style: TextStyle,
    pub decoration: RunDecoration,
    /// 字句の役割（コードブロックの着色。DEC-211）。
    ///
    /// **色そのものは持たない。** §16.10 のとおり、
    /// 実際の色は描画時にテーマで解決する。
    pub role: TokenRole,
    /// 行の左端からの相対 X
    pub x: f32,
    pub width: f32,
}

/// 1 行ぶんの配置。
#[derive(Debug, Clone, PartialEq)]
pub struct LineBox {
    /// ブロック先頭からの相対 Y
    pub top: f32,
    pub height: f32,
    /// 左端の位置（リストや引用の字下げを含む）
    pub left: f32,
    pub runs: Vec<TextRun>,
    /// この行が対応する原文のバイト範囲（ブロック内の相対位置）。
    ///
    /// **インライン解析を通った行では `None`。** 記法が消えるため、
    /// 表示上の文字と原文のバイトが 1 対 1 に対応しない。
    /// コードブロックと表は原文をそのまま出すので範囲を持つ。
    pub source: Option<Range<usize>>,
}

impl LineBox {
    /// 表示される文字列。試験と検索で使う。
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

/// ブロックのレイアウト結果。キャッシュの値でもある（§3.8）。
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutBlock {
    pub height: f32,
    pub lines: Vec<LineBox>,
    /// 引用の縦線を引くか
    pub quote_bar: bool,
    /// コードブロックの背景を敷くか
    pub code_background: bool,
    /// 水平線を引く位置（ブロック先頭からの相対 Y）。表の罫線に使う
    pub rules: Vec<f32>,
    /// 埋め込み（図・数式・画像）の置き場。無ければ `None`
    pub embed: Option<EmbedPlacement>,
}

/// 埋め込みを置く場所と大きさ。
///
/// **画素そのものは持たない。** 描画層がワーカーの結果を引いて描く。
/// レイアウト結果はキャッシュに載るため、ここに画素を持つとメモリを食う。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmbedPlacement {
    pub key: EmbedKey,
    /// ブロック先頭からの相対 Y
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// レイアウトの入力（§3.1）。
///
/// **ウィンドウや iced の型を入れない。** レイアウトを描画層から独立させ、
/// PDF 出力（用紙幅）からも同じ処理を呼べるようにするための担保である。
pub struct LayoutContext<'a> {
    pub width: f32,
    pub measurer: &'a dyn TextMeasurer,
    /// ブロックの上下余白
    pub block_spacing: f32,
    /// リスト・引用の字下げ 1 段ぶん
    pub indent_unit: f32,
    /// 相対パスの基準（文書の置き場）。画像の解決に使う
    pub base_dir: Option<&'a std::path::Path>,
    /// 埋め込みの描画状態を引く口。**読むだけ。**
    ///
    /// 依頼はアプリ層が行う（ワーカープールは `&mut` を要する）。
    /// レイアウトは「いま何が分かっているか」だけを見て箱の大きさを決める。
    pub embeds: Option<&'a dyn EmbedLookup>,
}

/// 埋め込みの状態を引く口。
pub trait EmbedLookup {
    fn state(&self, key: &EmbedKey) -> Option<JobState>;
}

/// ワーカープールをそのまま引き口として使う。
///
/// **包み直さない。** 包むとアプリ層で一時変数になり、描画要素より先に
/// 落ちて借用できない（実際に踏んだ）。
impl EmbedLookup for crate::embed::EmbedPool {
    fn state(&self, key: &EmbedKey) -> Option<JobState> {
        self.get(key).cloned()
    }
}

impl LayoutContext<'_> {
    fn style_for(kind: &BlockKind) -> TextStyle {
        match kind {
            BlockKind::Heading(level) => TextStyle::Heading(*level),
            BlockKind::Code { .. } => TextStyle::Mono,
            _ => TextStyle::Body,
        }
    }
}

/// インラインの見た目を、実際に使う字形と装飾へ写す。
fn resolve(base: TextStyle, span: SpanStyle) -> (TextStyle, RunDecoration) {
    match span {
        SpanStyle::Normal => (base, RunDecoration::None),
        // 見出しはもともと太いので、その中の太字は字形を変えない
        SpanStyle::Strong => match base {
            TextStyle::Heading(_) => (base, RunDecoration::None),
            _ => (TextStyle::Bold, RunDecoration::None),
        },
        // 字形は据え置き、印だけ付ける。見せ方は描画層が決める
        SpanStyle::Emphasis => (base, RunDecoration::Emphasis),
        SpanStyle::Code => (TextStyle::Mono, RunDecoration::Code),
        SpanStyle::Link => (base, RunDecoration::Link),
    }
}

/// ブロックをレイアウトする。`source` はそのブロックの原文。
pub fn layout_block(block: &Block, source: &str, cx: &LayoutContext) -> LaidOutBlock {
    let base = LayoutContext::style_for(&block.kind);
    let mut rules = Vec::new();

    let mut embed = None;

    // **箱を置けたときだけ行を捨てる。** 置けない場合（外部の画像など）は
    // 段落として描く。ここを取り違えると画面が空白になる（実際に踏んだ）
    let lines = if let Some(request) = embed_source(block, source, cx.width, cx.base_dir) {
        let (lines, placement) = layout_embed(&request, base, cx);
        embed = placement;
        lines
    } else {
        match block.kind {
            // **コードは折り返さずに出す**（§3.5）。横スクロールで見せる
            BlockKind::Code { ref language } => layout_code(source, language.as_deref(), base, cx),
            BlockKind::Table => layout_table(&parse_table(source), base, cx, &mut rules),
            _ => layout_content(&parse_block(source, &block.kind), base, cx),
        }
    };

    let bottom = lines
        .iter()
        .map(|line| line.top + line.height)
        .fold(0.0_f32, f32::max)
        .max(embed.map_or(0.0, |box_| box_.top + box_.height));
    // 罫線は最後の行の下にあるので、高さに含める
    let height = rules
        .last()
        .copied()
        .unwrap_or(0.0)
        .max(bottom)
        // 空のブロックでも 1 行ぶんの高さは確保する
        .max(if lines.is_empty() {
            cx.measurer.line_height(base)
        } else {
            0.0
        });

    LaidOutBlock {
        height: height + cx.block_spacing,
        lines,
        quote_bar: matches!(block.kind, BlockKind::Quote),
        // 埋め込みは地を敷かない。図の背景と重なって見づらい
        code_background: matches!(block.kind, BlockKind::Code { .. }) && embed.is_none(),
        rules,
        embed,
    }
}

/// このブロックが埋め込みになりうるか。**本文を読まずに判定する。**
///
/// レイアウトキャッシュを引く前に呼ぶので、ロープから文字列を切り出す前に
/// 分かる必要がある。
pub fn is_embed_block(block: &Block, source: &str) -> bool {
    match &block.kind {
        BlockKind::Code { language } => language
            .as_deref()
            .map(embed_kind_of)
            .unwrap_or(None)
            .is_some(),
        // **安く判定する。** ここはレイアウトキャッシュを引く前に呼ばれる。
        // 本当に画像かどうかは `embed_source` が comrak で確かめる
        // `<img>` タグ 1 つの段落も画像になりうる（v2.1.0 R-17）。
        // **ここで見落とすと、描く依頼が出ず「描画中」のまま止まる**（実際に踏んだ）
        BlockKind::Paragraph => {
            let head = source.trim_start();
            head.starts_with("![")
                || head
                    .get(..4)
                    .is_some_and(|tag| tag.eq_ignore_ascii_case("<img"))
        }
        _ => false,
    }
}

fn embed_kind_of(language: &str) -> Option<EmbedKind> {
    match language.trim().to_ascii_lowercase().as_str() {
        "mermaid" => Some(EmbedKind::Diagram),
        "math" | "latex" => Some(EmbedKind::Math),
        _ => None,
    }
}

/// このブロックが埋め込みなら、その描画依頼を返す。
///
/// **判定をここに 1 か所だけ置く。** アプリ層（依頼する側）とレイアウト層
/// （箱を置く側）が別々に判定すると、片方だけ直したときに食い違う。
pub fn embed_source(
    block: &Block,
    source: &str,
    width: f32,
    base_dir: Option<&std::path::Path>,
) -> Option<EmbedSource> {
    if matches!(block.kind, BlockKind::Paragraph) {
        return image_source(source, width, base_dir);
    }
    let BlockKind::Code { language } = &block.kind else {
        return None;
    };
    let kind = embed_kind_of(language.as_deref()?)?;
    let (_, body) = code_body(source);

    // 描けない図は原文を見せる（OPEN-210。判断は `embed::diagram` が持つ）
    if kind == EmbedKind::Diagram && !crate::embed::diagram::is_supported(body) {
        return None;
    }

    Some(EmbedSource::new(kind, body, width))
}

/// 画像だけの段落なら、その描画依頼を返す。
///
/// **段落全体が画像 1 つのときだけ扱う。** 文章の途中にある画像は、
/// 行分割の中に箱を置く必要があり、現状は文字の目印で代用する（DD-OPEN-13）。
///
/// 判定は comrak に任せる。自前で `![...](...)` を切り出すと、
/// 題名つきや括弧を含む URL で崩れる。
fn image_source(
    source: &str,
    width: f32,
    base_dir: Option<&std::path::Path>,
) -> Option<EmbedSource> {
    // **判定は解析層に一本化する。** ここで行の中身も見ると、
    // 解析層の「画像だけの段落では行を空にする」と食い違う（実際に踏んだ）
    let content = parse_block(source, &BlockKind::Paragraph);
    let reference = content.lone_image.as_deref()?;

    // **ネットワークは最初から取りに行かない**（§16.12）
    if crate::embed::is_remote(reference) {
        return None;
    }
    let path = crate::embed::resolve(reference, base_dir);
    // **更新時刻も鍵に入れる**（DD-OPEN-16）。
    // パスだけだと、画像を差し替えても古いものが出続ける
    let stamp = crate::embed::stamp_of(&path);
    // `<img width="…">` なら、その幅より大きくしない（画面の幅は超えない。v2.1.0）
    let width = content
        .lone_image_width
        .map_or(width, |wanted| wanted.min(width));
    Some(EmbedSource::new(EmbedKind::Image, path.to_string_lossy(), width).with_stamp(stamp))
}

/// 埋め込みを 1 つ置く（§16.5）。
///
/// 結果が届くまでは**寸法が確定したプレースホルダ**を置く。
/// 届いたら実寸へ置き換わり、高さ索引が直る（§3.7 のアンカー補正）。
fn layout_embed(
    request: &EmbedSource,
    base: TextStyle,
    cx: &LayoutContext,
) -> (Vec<LineBox>, Option<EmbedPlacement>) {
    let key = request.key();
    let state = cx.embeds.and_then(|lookup| lookup.state(&key));
    let line_height = cx.measurer.line_height(base);

    let label = |text: &str| {
        let width = cx.measurer.width(text, base);
        vec![LineBox {
            top: 0.0,
            height: line_height,
            left: 0.0,
            runs: vec![TextRun {
                text: text.to_owned(),
                style: base,
                decoration: RunDecoration::None,
                role: TokenRole::Plain,
                x: 0.0,
                width,
            }],
            source: None,
        }]
    };

    let name = match request.kind {
        EmbedKind::Diagram => "図",
        EmbedKind::Math => "数式",
        EmbedKind::Image => "画像",
    };

    match state {
        Some(JobState::Done(embed)) => {
            let height = embed.display_height(cx.width);
            (
                Vec::new(),
                Some(EmbedPlacement {
                    key,
                    top: 0.0,
                    width: cx.width,
                    height,
                }),
            )
        }
        // **失敗は画面に出す。** 黙って空白にすると、利用者は
        // 図を書き間違えたことに気づけない
        Some(JobState::Failed(error)) => (label(&format!("{name}: {error}")), None),
        // 依頼済み、または未依頼。どちらも待ちとして扱う
        _ => (
            label(&format!("{name}を描画中…")),
            Some(EmbedPlacement {
                key,
                top: line_height,
                width: cx.width,
                height: PLACEHOLDER_HEIGHT - line_height,
            }),
        ),
    }
}

/// セルの左右の余白。
const CELL_PADDING: f32 = 8.0;

/// 列幅を測るために見る行数（§3.3）。
///
/// **表全体を見ると数千行で重い。** 先頭だけ見て決め、以降の行がはみ出す場合は
/// そのセルの中で折り返す。
const MAX_MEASURED_ROWS: usize = 200;

/// 表をレイアウトする（§3.3）。
fn layout_table(
    table: &TableContent,
    base: TextStyle,
    cx: &LayoutContext,
    rules: &mut Vec<f32>,
) -> Vec<LineBox> {
    let columns = table.columns();
    if columns == 0 {
        return Vec::new();
    }

    let (natural, minimum) = measure_columns(table, base, cx);
    let widths = distribute(&natural, &minimum, cx.width);

    // 列の左端。セルの余白は幅の内側に取る
    let mut origins = Vec::with_capacity(columns);
    let mut x = 0.0_f32;
    for width in &widths {
        origins.push(x);
        x += width;
    }

    let line_height = cx.measurer.line_height(base);
    let mut lines = Vec::new();
    let mut top = 0.0_f32;

    for row in &table.rows {
        // 見出しは太字。行の高さは**いちばん背の高いセル**に合わせる
        let style = if row.header { TextStyle::Bold } else { base };
        let mut wrapped = Vec::with_capacity(columns);
        let mut tallest = 1usize;

        for (column, width) in widths.iter().enumerate() {
            // **セルが足りない行がありうる。** 空として扱い、列をずらさない
            let spans = row
                .cells
                .get(column)
                .map(|cell| cell.spans.as_slice())
                .unwrap_or(&[]);
            let inner = (width - CELL_PADDING * 2.0).max(1.0);
            let cell = wrap_spans(spans, style, inner, cx.measurer);
            tallest = tallest.max(cell.len().max(1));
            wrapped.push(cell);
        }

        for (column, (cell, (&width, &origin))) in wrapped
            .into_iter()
            .zip(widths.iter().zip(origins.iter()))
            .enumerate()
        {
            let inner = (width - CELL_PADDING * 2.0).max(1.0);
            for (index, runs) in cell.into_iter().enumerate() {
                if runs.is_empty() {
                    continue;
                }
                let used: f32 = runs.iter().map(|run| run.width).sum();
                let slack = (inner - used).max(0.0);
                let offset = match table.align(column) {
                    CellAlign::Left => 0.0,
                    CellAlign::Center => slack / 2.0,
                    CellAlign::Right => slack,
                };
                lines.push(LineBox {
                    top: top + index as f32 * line_height,
                    height: line_height,
                    left: origin + CELL_PADDING + offset,
                    runs,
                    source: None,
                });
            }
        }

        top += tallest as f32 * line_height;
        rules.push(top);
    }

    lines
}

/// 各列の自然幅と最小幅を測る（§3.3 パス 1）。
fn measure_columns(
    table: &TableContent,
    base: TextStyle,
    cx: &LayoutContext,
) -> (Vec<f32>, Vec<f32>) {
    let columns = table.columns();
    let mut natural = vec![0.0_f32; columns];
    let mut minimum = vec![0.0_f32; columns];

    for row in table.rows.iter().take(MAX_MEASURED_ROWS) {
        let style = if row.header { TextStyle::Bold } else { base };
        for (column, cell) in row.cells.iter().enumerate() {
            let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
            if text.is_empty() {
                continue;
            }
            let whole = cx.measurer.width(&text, style) + CELL_PADDING * 2.0;
            natural[column] = natural[column].max(whole);
            minimum[column] =
                minimum[column].max(narrowest(&text, style, cx.measurer) + CELL_PADDING * 2.0);
        }
    }

    // 空の列でも潰さない
    let floor = cx.measurer.width("W", base) + CELL_PADDING * 2.0;
    for (least, widest) in minimum.iter_mut().zip(natural.iter_mut()) {
        *least = least.max(floor);
        *widest = widest.max(*least);
    }
    (natural, minimum)
}

/// これ以上縮められない幅。
///
/// UAX #14 の改行可能位置で切った断片のうち、いちばん広いもの。
/// **日本語は 1 文字、英数は最長の単語**になる。
fn narrowest(text: &str, style: TextStyle, measurer: &dyn TextMeasurer) -> f32 {
    let mut widest = 0.0_f32;
    let mut start = 0usize;
    for (index, _) in linebreaks(text) {
        if index > start {
            widest = widest.max(measurer.width(text[start..index].trim_end(), style));
            start = index;
        }
    }
    widest
}

/// 列幅を決める（§3.3 パス 2）。
fn distribute(natural: &[f32], minimum: &[f32], available: f32) -> Vec<f32> {
    let total_natural: f32 = natural.iter().sum();
    if total_natural <= available {
        // 収まるなら自然幅のまま。表を左寄せし、余白は右に残す
        return natural.to_vec();
    }

    let total_minimum: f32 = minimum.iter().sum();
    if total_minimum >= available {
        // **これ以上縮められない。** 最小幅のまま置き、横スクロールで見せる
        return minimum.to_vec();
    }

    // 余った幅を「縮められる量」に比例して配る
    let slack = available - total_minimum;
    let shrinkable: f32 = natural
        .iter()
        .zip(minimum)
        .map(|(widest, least)| widest - least)
        .sum();
    if shrinkable <= 0.0 {
        return minimum.to_vec();
    }

    natural
        .iter()
        .zip(minimum)
        .map(|(widest, least)| least + slack * (widest - least) / shrinkable)
        .collect()
}

/// インライン要素の列を、与えられた幅で折り返す。
fn wrap_spans(
    spans: &[Span],
    style: TextStyle,
    available: f32,
    measurer: &dyn TextMeasurer,
) -> Vec<Vec<TextRun>> {
    let logical = LogicalLine {
        spans: spans.to_vec(),
        indent: 0,
        marker: None,
    };
    let styled = StyledText::build(&logical, style);
    if styled.text.is_empty() {
        return Vec::new();
    }
    wrap_styled(&styled, measurer, &|_| available)
        .into_iter()
        .map(|range| styled.runs(range, measurer))
        .collect()
}

/// コードブロック。**フェンス行は出さず**、中身を着色して 1 行ずつ置く。
///
/// 記法を消すのはインライン要素と同じ考え方である（§12.7）。
/// ``` の行を見せても読み手の役に立たない。
fn layout_code(
    source: &str,
    language: Option<&str>,
    style: TextStyle,
    cx: &LayoutContext,
) -> Vec<LineBox> {
    let (offset, body) = code_body(source);
    let line_height = cx.measurer.line_height(style);
    let mut lines = Vec::new();
    let mut top = 0.0_f32;

    // **着色はここで呼ぶ。** レイアウトキャッシュ（§3.8）の内側なので、
    // ブロックの revision が変わらない限り再着色は起きない（DEC-211）
    let highlighted = highlight(body, language);

    for ((start, raw), tokens) in source_lines(body).zip(highlighted) {
        let text = raw.trim_end_matches(['\n', '\r']);
        let mut runs = Vec::with_capacity(tokens.len());
        let mut x = 0.0_f32;

        for token in tokens {
            let Some(part) = text.get(token.range.clone()) else {
                continue;
            };
            if part.is_empty() {
                continue;
            }
            let width = cx.measurer.width(part, style);
            runs.push(TextRun {
                text: part.to_owned(),
                style,
                decoration: RunDecoration::None,
                role: token.role,
                x,
                width,
            });
            x += width;
        }

        lines.push(LineBox {
            top,
            height: line_height,
            left: 0.0,
            runs,
            source: Some(offset + start..offset + start + raw.len()),
        });
        top += line_height;
    }
    lines
}

/// フェンスを除いた中身と、その開始位置を返す。
///
/// **閉じフェンスが無いことがある**（文書末尾で開いたまま。§12.3）。
fn code_body(source: &str) -> (usize, &str) {
    let is_fence = |line: &str| {
        let trimmed = line.trim_start();
        trimmed.starts_with("```") || trimmed.starts_with("~~~")
    };

    let mut start = 0usize;
    let mut end = source.len();

    if let Some((_, first)) = source_lines(source).next() {
        if is_fence(first) {
            start = first.len();
        }
    }
    // 最後の行が閉じフェンスなら落とす
    if let Some((last_start, last)) = source_lines(source).last() {
        if last_start >= start && is_fence(last) {
            end = last_start;
        }
    }
    if start > end {
        return (0, "");
    }
    (start, &source[start..end])
}

/// インライン解析の結果を並べる。
///
/// **字下げはここで足さない。** 引用もリストの入れ子も、解析側が段数として
/// 返している（`LogicalLine::indent`）。ここで種類を見て足すと二重に効く。
fn layout_content(content: &BlockContent, base: TextStyle, cx: &LayoutContext) -> Vec<LineBox> {
    let mut lines = Vec::new();
    let mut top = 0.0_f32;

    for logical in &content.lines {
        let indent = f32::from(logical.indent) * cx.indent_unit;
        top = place(logical, base, indent, top, cx, &mut lines);
    }
    lines
}

/// 論理行 1 つを折り返して並べ、次の `top` を返す。
fn place(
    logical: &LogicalLine,
    base: TextStyle,
    indent: f32,
    mut top: f32,
    cx: &LayoutContext,
    out: &mut Vec<LineBox>,
) -> f32 {
    let styled = StyledText::build(logical, base);
    if styled.text.is_empty() {
        return top;
    }

    // 行頭記号のぶんだけ 2 行目以降を下げる（ぶら下げ字下げ）
    let marker_width = logical
        .marker
        .as_deref()
        .map(|marker| cx.measurer.width(marker, base))
        .unwrap_or(0.0);

    // 1 文字も置けない幅にはしない。折り返しが止まらなくなる
    let floor = cx.measurer.width("W", base);
    let first = (cx.width - indent).max(floor);
    let rest = (cx.width - indent - marker_width).max(floor);

    let ranges = wrap_styled(&styled, cx.measurer, &|index| {
        if index == 0 {
            first
        } else {
            rest
        }
    });

    for (index, range) in ranges.iter().enumerate() {
        let runs = styled.runs(range.clone(), cx.measurer);
        let height = runs
            .iter()
            .map(|run| cx.measurer.line_height(run.style))
            .fold(cx.measurer.line_height(base), f32::max);

        out.push(LineBox {
            top,
            height,
            left: if index == 0 {
                indent
            } else {
                indent + marker_width
            },
            runs,
            source: None,
        });
        top += height;
    }
    top
}

/// 行頭記号とインライン要素を 1 本の文字列につないだもの。
///
/// 折り返しは**つないだ文字列の上で**行う。区間ごとに折ると、
/// `**太**字` のような境界で不自然に切れる。
struct StyledText {
    text: String,
    /// (範囲, 字形, 装飾)。範囲は `text` のバイト位置で、隙間なく連続する
    pieces: Vec<(Range<usize>, TextStyle, RunDecoration)>,
}

impl StyledText {
    fn build(logical: &LogicalLine, base: TextStyle) -> Self {
        let mut text = String::new();
        let mut pieces = Vec::new();

        let mut push = |part: &str, style: TextStyle, decoration: RunDecoration| {
            if part.is_empty() {
                return;
            }
            let start = text.len();
            text.push_str(part);
            pieces.push((start..text.len(), style, decoration));
        };

        if let Some(marker) = &logical.marker {
            push(marker, base, RunDecoration::None);
        }
        for span in &logical.spans {
            let (style, decoration) = resolve(base, span.style);
            push(&span.text, style, decoration);
        }

        Self { text, pieces }
    }

    /// `range` の幅。またがる区間ごとに測って足す。
    fn width(&self, range: Range<usize>, measurer: &dyn TextMeasurer) -> f32 {
        let mut total = 0.0;
        for (piece, style, _) in &self.pieces {
            let start = piece.start.max(range.start);
            let end = piece.end.min(range.end);
            if start < end {
                total += measurer.width(&self.text[start..end], *style);
            }
        }
        total
    }

    /// `range` を、区間の境界で切り分けた描画単位にする。
    fn runs(&self, range: Range<usize>, measurer: &dyn TextMeasurer) -> Vec<TextRun> {
        let mut runs: Vec<TextRun> = Vec::new();
        let mut x = 0.0_f32;

        for (piece, style, decoration) in &self.pieces {
            let start = piece.start.max(range.start);
            let end = piece.end.min(range.end);
            if start >= end {
                continue;
            }
            let part = &self.text[start..end];

            // 折り返しで生まれた行頭の空白は落とす（行が下がって見えるため）
            let part = if runs.is_empty() && range.start > 0 {
                part.trim_start_matches(' ')
            } else {
                part
            };
            if part.is_empty() {
                continue;
            }

            // 同じ見た目が続くならつなぐ。描画の回数を減らす
            if let Some(last) = runs.last_mut() {
                if last.style == *style && last.decoration == *decoration {
                    last.text.push_str(part);
                    let width = measurer.width(part, *style);
                    last.width += width;
                    x += width;
                    continue;
                }
            }

            let width = measurer.width(part, *style);
            runs.push(TextRun {
                text: part.to_owned(),
                style: *style,
                decoration: *decoration,
                role: TokenRole::Plain,
                x,
                width,
            });
            x += width;
        }
        runs
    }
}

/// 改行を含めて 1 行ずつ、開始オフセットとともに返す。
fn source_lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = source;
    let mut offset = 0usize;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let (line, tail) = match rest.find('\n') {
            Some(index) => rest.split_at(index + 1),
            None => (rest, ""),
        };
        let start = offset;
        offset += line.len();
        rest = tail;
        Some((start, line))
    })
}

/// 装飾つきの 1 行を折り返す。`available` は行番号ごとの利用可能幅。
fn wrap_styled(
    styled: &StyledText,
    measurer: &dyn TextMeasurer,
    available: &dyn Fn(usize) -> f32,
) -> Vec<Range<usize>> {
    wrap_ranges(&styled.text, available, &|range| {
        styled.width(range, measurer)
    })
}

/// 1 行を利用可能幅で折り返す（§3.2）。
///
/// 改行可能位置は **UAX #14**（`unicode-linebreak`）で求める。
/// 日本語の基本的な禁則——`、` `。` `）` `」` を行頭に置かない、
/// `（` `「` を行末に置かない——はこの規則に含まれる。
fn wrap_ranges(
    text: &str,
    available: &dyn Fn(usize) -> f32,
    width_of: &dyn Fn(Range<usize>) -> f32,
) -> Vec<Range<usize>> {
    // 折る必要が無い場合（空行・幅に収まる行）は 1 行として返す。
    //
    // clippy の single_range_in_vec_init は `vec![0..n]` を
    // 「`vec![0; n]` の書き間違いでは」と疑うが、ここは**本当に範囲 1 件が欲しい**。
    #[allow(clippy::single_range_in_vec_init)]
    {
        if text.is_empty() {
            return vec![0..0];
        }
        if width_of(0..text.len()) <= available(0) {
            return vec![0..text.len()];
        }
    }

    // UAX #14 の改行可能位置（バイト位置）。
    //
    // **末尾の位置を落としてはいけない。** `linebreaks` は本文の終端を
    // Mandatory として返すが、これを除くと最後の区切りが本文末尾へ届かず、
    // 余分な 1 行が生まれる（実際に踏んだ）。
    let opportunities: Vec<usize> = linebreaks(text)
        .map(|(index, kind)| {
            debug_assert!(
                kind != BreakOpportunity::Mandatory || index == text.len(),
                "行内に必須改行があるのは想定外（行単位で分割済みのはず）"
            );
            index
        })
        .collect();

    let mut result = Vec::new();
    let mut start = 0usize;

    while start < text.len() {
        let limit = available(result.len());

        // start から先で、幅に収まる最も後ろの改行可能位置を探す
        let mut chosen = None;
        for &candidate in opportunities.iter() {
            if candidate <= start {
                continue;
            }
            if width_of(start..candidate) <= limit {
                chosen = Some(candidate);
            } else {
                break;
            }
        }

        let end = match chosen {
            Some(end) => end,
            // **どこでも折れない場合は文字単位で強制的に折る**（§3.2 の 6）。
            // 長い英単語や URL で起きる
            None => force_break(text, start, limit, width_of),
        };

        result.push(start..end);
        start = end;
    }

    if result.is_empty() {
        result.push(0..text.len());
    }
    result
}

/// 改行可能位置が無いときに、文字境界で強制的に折る位置を返す。
fn force_break(
    text: &str,
    start: usize,
    available: f32,
    width_of: &dyn Fn(Range<usize>) -> f32,
) -> usize {
    let mut last = start;
    for (index, ch) in text[start..].char_indices() {
        let end = start + index + ch.len_utf8();
        if width_of(start..end) > available {
            // 1 文字も入らない場合でも必ず 1 文字は進める（無限ループを避ける）
            return if last == start { end } else { last };
        }
        last = end;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::measure::FixedMeasurer;
    use crate::parse::scan_lines;

    fn context(width: f32, measurer: &FixedMeasurer) -> LayoutContext<'_> {
        LayoutContext {
            width,
            measurer,
            block_spacing: 12.0,
            indent_unit: 24.0,
            base_dir: None,
            embeds: None,
        }
    }

    fn layout_first(text: &str, width: f32) -> LaidOutBlock {
        let measurer = FixedMeasurer::default();
        let cx = context(width, &measurer);
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        layout_block(block, &text[block.bytes.clone()], &cx)
    }

    /// 単一の見た目で折り返す。既存の試験のための入口。
    fn wrap_line(
        text: &str,
        available: f32,
        measurer: &dyn TextMeasurer,
        style: TextStyle,
    ) -> Vec<Range<usize>> {
        wrap_ranges(text, &|_| available, &|range| {
            measurer.width(&text[range], style)
        })
    }

    fn rendered(result: &LaidOutBlock) -> String {
        result
            .lines
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn short_paragraph_is_one_line() {
        let result = layout_first("短い段落\n", 800.0);
        assert_eq!(result.lines.len(), 1);
    }

    /// **記法は表示されない。** プレビューが原文の整形ではなくなった要点。
    #[test]
    fn markup_is_not_rendered() {
        let result = layout_first("これは **太字** です\n", 800.0);
        assert_eq!(rendered(&result), "これは 太字 です");
    }

    #[test]
    fn strong_uses_bold_glyphs() {
        let result = layout_first("ふつう **太字**\n", 800.0);
        let styles: Vec<_> = result.lines[0].runs.iter().map(|run| run.style).collect();
        assert!(styles.contains(&TextStyle::Bold), "{styles:?}");
    }

    /// 強調は字形を据え置き、装飾の印だけ付ける。見せ方は描画層が決める。
    #[test]
    fn emphasis_is_marked_not_slanted() {
        let result = layout_first("ふつう *強調*\n", 800.0);
        let run = result.lines[0]
            .runs
            .iter()
            .find(|run| run.decoration == RunDecoration::Emphasis)
            .expect("強調の区間がある");
        assert_eq!(run.text, "強調");
        assert_eq!(run.style, TextStyle::Body);
    }

    #[test]
    fn inline_code_uses_mono() {
        let result = layout_first("設定は `a.toml` です\n", 800.0);
        let run = result.lines[0]
            .runs
            .iter()
            .find(|run| run.decoration == RunDecoration::Code)
            .expect("コードの区間がある");
        assert_eq!(run.text, "a.toml");
        assert_eq!(run.style, TextStyle::Mono);
    }

    #[test]
    fn link_shows_text_only() {
        let result = layout_first("詳しくは [設計書](design/a.md) を見よ\n", 800.0);
        assert_eq!(rendered(&result), "詳しくは 設計書 を見よ");
        assert!(result.lines[0]
            .runs
            .iter()
            .any(|run| run.decoration == RunDecoration::Link));
    }

    #[test]
    fn heading_hashes_are_removed() {
        let result = layout_first("## 小見出し\n", 800.0);
        assert_eq!(rendered(&result), "小見出し");
        assert_eq!(result.lines[0].runs[0].style, TextStyle::Heading(2));
    }

    /// リストは行頭記号が付き、項目ごとに 1 行になる。
    #[test]
    fn list_items_get_markers() {
        let result = layout_first("- 項目 A\n- 項目 B\n", 800.0);
        assert_eq!(rendered(&result), "• 項目 A\n• 項目 B");
    }

    #[test]
    fn ordered_list_is_numbered() {
        let result = layout_first("1. 一つ目\n2. 二つ目\n", 800.0);
        assert_eq!(rendered(&result), "1. 一つ目\n2. 二つ目");
    }

    /// 2 行目以降は行頭記号のぶんぶら下げる。
    #[test]
    fn wrapped_list_item_hangs() {
        let text = format!("- {}\n", "あ".repeat(40));
        let result = layout_first(&text, 200.0);
        assert!(result.lines.len() >= 2);
        assert!(
            result.lines[1].left > result.lines[0].left,
            "{} > {}",
            result.lines[1].left,
            result.lines[0].left
        );
    }

    #[test]
    fn long_paragraph_wraps() {
        // 半角 8px・幅 160px → 1 行 20 桁。全角は 2 桁ぶん
        let text = format!("{}\n", "あ".repeat(30));
        let result = layout_first(&text, 160.0);
        assert_eq!(result.lines.len(), 3, "30 文字 = 60 桁 → 20 桁ずつで 3 行");
    }

    #[test]
    fn narrower_width_produces_more_lines() {
        let text = format!("{}\n", "日本語の段落。".repeat(10));
        let wide = layout_first(&text, 800.0).lines.len();
        let narrow = layout_first(&text, 200.0).lines.len();
        assert!(narrow > wide, "{narrow} > {wide}");
    }

    /// 折り返しは**装飾の境界をまたいで**効く。区間ごとに折ってはいけない。
    #[test]
    fn wrapping_crosses_style_boundaries() {
        let text = format!("{} **{}**\n", "あ".repeat(15), "い".repeat(15));
        let result = layout_first(&text, 160.0);
        // 記法を除いた表示文字は 31 文字 = 62 桁 → 20 桁ずつ
        let flat: String = rendered(&result).replace('\n', "");
        assert_eq!(flat.chars().filter(|c| *c == 'あ').count(), 15);
        assert_eq!(flat.chars().filter(|c| *c == 'い').count(), 15);
        assert!(result.lines.len() >= 3, "{}", result.lines.len());
    }

    /// 行頭禁則: `、` `。` を行頭に置かない（UAX #14）
    #[test]
    fn does_not_start_line_with_japanese_punctuation() {
        let measurer = FixedMeasurer::default();
        let text = "あいうえお、かきくけこ";
        for width in [80.0, 96.0, 112.0] {
            for range in wrap_line(text, width, &measurer, TextStyle::Body) {
                let segment = &text[range];
                assert!(
                    !segment.starts_with('、') && !segment.starts_with('。'),
                    "幅 {width} で行頭に約物が来た: {segment:?}"
                );
            }
        }
    }

    /// 行末禁則: `（` `「` を行末に置かない
    #[test]
    fn does_not_end_line_with_opening_bracket() {
        let measurer = FixedMeasurer::default();
        let text = "あいうえお（かきくけこ）";
        for width in [80.0, 96.0, 112.0] {
            for range in wrap_line(text, width, &measurer, TextStyle::Body) {
                let segment = &text[range];
                assert!(
                    !segment.ends_with('（') && !segment.ends_with('「'),
                    "幅 {width} で行末に括弧が来た: {segment:?}"
                );
            }
        }
    }

    /// 折れない長い文字列は文字単位で強制的に折る（§3.2 の 6）
    #[test]
    fn breaks_long_unbreakable_text() {
        let measurer = FixedMeasurer::default();
        let text = "a".repeat(100);
        let ranges = wrap_line(&text, 80.0, &measurer, TextStyle::Body);
        assert!(ranges.len() >= 10, "強制的に折られること: {}", ranges.len());
        // 全範囲が連続して元の文字列を覆うこと
        let mut cursor = 0;
        for range in &ranges {
            assert_eq!(range.start, cursor);
            cursor = range.end;
        }
        assert_eq!(cursor, text.len());
    }

    /// 極端に狭くても無限ループしない
    #[test]
    fn extremely_narrow_width_terminates() {
        let measurer = FixedMeasurer::default();
        let ranges = wrap_line("あいうえお", 1.0, &measurer, TextStyle::Body);
        assert_eq!(ranges.len(), 5, "1 文字ずつになる");
    }

    #[test]
    fn extremely_narrow_width_terminates_in_block() {
        let result = layout_first("あいうえお かきくけこ\n", 1.0);
        assert!(!result.lines.is_empty());
    }

    /// コードは折り返さない（§3.5）。横スクロールで見せる。
    #[test]
    fn code_does_not_wrap() {
        let long = "x".repeat(500);
        let text = format!("```\n{long}\n```\n");
        let result = layout_first(&text, 200.0);
        assert_eq!(result.lines.len(), 1, "本文 1 行のまま折り返さない");
        assert!(result.code_background);
    }

    /// **フェンス行は出さない。** 記法を消すのはインライン要素と同じ考え方である。
    #[test]
    fn code_fences_are_hidden() {
        let result = layout_first("```rust\nlet a = 1;\n```\n", 800.0);
        assert_eq!(rendered(&result), "let a = 1;");
    }

    /// 閉じフェンスが無くても落ちない（文書末尾で開いたまま。§12.3）。
    #[test]
    fn unterminated_fence_is_safe() {
        let result = layout_first("```rust\nlet a = 1;\n", 800.0);
        assert_eq!(rendered(&result), "let a = 1;");
    }

    /// コードの中では Markdown の記法を解釈しない。
    #[test]
    fn code_keeps_markup_as_is() {
        let text = "```\nlet a = **b**;\n```\n";
        let result = layout_first(text, 800.0);
        assert!(rendered(&result).contains("**b**"), "{}", rendered(&result));
    }

    /// 言語が分かれば着色する（DEC-211）。
    #[test]
    fn known_language_is_highlighted() {
        let result = layout_first("```rust\nfn main() {}\n```\n", 800.0);
        let roles: Vec<_> = result.lines[0].runs.iter().map(|run| run.role).collect();
        assert!(roles.contains(&TokenRole::Keyword), "{roles:?}");
    }

    /// 言語が無ければ素のまま。**着色しないだけで、表示は変わらない。**
    #[test]
    fn unknown_language_is_plain() {
        let result = layout_first("```\nfn main() {}\n```\n", 800.0);
        assert_eq!(rendered(&result), "fn main() {}");
        assert!(result.lines[0]
            .runs
            .iter()
            .all(|run| run.role == TokenRole::Plain));
    }

    /// 着色しても表示される文字は原文と同じでなければならない。
    #[test]
    fn highlighting_does_not_change_the_text() {
        let code = "fn main() {\n    let s = \"hi\"; // コメント\n}";
        let text = format!("```rust\n{code}\n```\n");
        let result = layout_first(&text, 800.0);
        assert_eq!(rendered(&result), code);
    }

    #[test]
    fn quote_is_indented_and_marked() {
        let result = layout_first("> 引用です\n", 800.0);
        assert!(result.quote_bar);
        assert_eq!(rendered(&result), "引用です");
        assert_eq!(result.lines[0].left, 24.0);
    }

    #[test]
    fn heading_is_taller_than_paragraph() {
        let heading = layout_first("# 見出し\n", 800.0).height;
        let paragraph = layout_first("見出し\n", 800.0).height;
        assert!(heading > paragraph);
    }

    /// コードの各行は原文範囲を持ち、連続して**中身**を覆う。
    ///
    /// フェンス行は出さないので、範囲は開きフェンスの直後から始まる。
    /// インライン解析を通る行は記法が消えるため範囲を持たない（`source` は `None`）。
    #[test]
    fn code_line_source_ranges_cover_the_body() {
        let text = "```\nabc\ndef\n```\n";
        let measurer = FixedMeasurer::default();
        let cx = context(200.0, &measurer);
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        let source = &text[block.bytes.clone()];
        let result = layout_block(block, source, &cx);

        let opening = "```\n".len();
        let mut cursor = opening;
        for line in &result.lines {
            let range = line.source.clone().expect("コードは原文範囲を持つ");
            assert_eq!(range.start, cursor, "行の範囲が連続していない");
            cursor = range.end;
            // 文字境界に乗っていること
            let _ = &source[range];
        }
        assert_eq!(
            cursor,
            source.len() - "```\n".len(),
            "閉じフェンスの手前まで"
        );
    }

    #[test]
    fn paragraph_lines_have_no_source_range() {
        let result = layout_first("**太字**の段落\n", 800.0);
        assert!(result.lines[0].source.is_none());
    }

    /// 行内の区間は左から隙間なく並ぶ。
    #[test]
    fn runs_are_laid_out_left_to_right() {
        let result = layout_first("あ **い** う\n", 800.0);
        let mut x = 0.0_f32;
        for run in &result.lines[0].runs {
            assert!((run.x - x).abs() < 0.01, "{} != {}", run.x, x);
            x += run.width;
        }
        assert!(x > 0.0);
    }

    const TABLE: &str = "| 名前 | 値 |
|---|---:|
| あ | 1 |
| い | 22 |
";

    /// 見出し 1 行 + 本体 2 行ぶんの罫線が引かれる。
    #[test]
    fn table_has_a_rule_per_row() {
        let result = layout_first(TABLE, 800.0);
        assert_eq!(result.rules.len(), 3);
        // 罫線は単調に増える
        for pair in result.rules.windows(2) {
            assert!(pair[1] > pair[0], "{:?}", result.rules);
        }
    }

    /// 2 列目は 1 列目の幅より右から始まる。
    #[test]
    fn second_column_starts_after_the_first() {
        let result = layout_first(TABLE, 800.0);
        let lefts: Vec<f32> = result.lines.iter().map(|line| line.left).collect();
        // 1 列目は余白ぶんだけ内側
        assert_eq!(lefts.iter().cloned().fold(f32::MAX, f32::min), CELL_PADDING);
        // 「名前」= 全角 2 文字 = 32px、余白 16px なので 1 列目は 48px 幅
        assert!(
            lefts.iter().any(|x| *x >= 48.0 + CELL_PADDING),
            "2 列目が始まっていない: {lefts:?}"
        );
    }

    /// 見出しは太字になる。
    #[test]
    fn table_header_is_bold() {
        let result = layout_first(TABLE, 800.0);
        assert_eq!(result.lines[0].runs[0].style, TextStyle::Bold);
        assert_eq!(result.lines[0].runs[0].text, "名前");
    }

    /// 記法はセルの中でも消える。
    #[test]
    fn table_cells_render_inline_markup() {
        let text = "| a |
|---|
| **太字** |
";
        let result = layout_first(text, 800.0);
        let bold = result
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .find(|run| run.text == "太字")
            .expect("セルの中身がある");
        assert_eq!(bold.style, TextStyle::Bold);
    }

    /// 自然幅で収まるなら、そのまま置いて余白は右に残す（§3.3 パス 2）。
    #[test]
    fn narrow_table_keeps_natural_widths() {
        let wide = layout_first(TABLE, 800.0);
        let wider = layout_first(TABLE, 1600.0);
        let lefts: Vec<f32> = wide.lines.iter().map(|line| line.left).collect();
        let lefts2: Vec<f32> = wider.lines.iter().map(|line| line.left).collect();
        assert_eq!(
            lefts, lefts2,
            "収まっている間は幅を変えても列位置が動かない"
        );
    }

    /// 収まらないときは縮める。縮めた結果は利用可能幅に収まる。
    #[test]
    fn overflowing_table_shrinks_to_fit() {
        let text = format!(
            "| {} | {} |
|---|---|
| a | b |
",
            "あ".repeat(40),
            "い".repeat(40)
        );
        let result = layout_first(&text, 400.0);
        let rightmost = result
            .lines
            .iter()
            .flat_map(|line| {
                line.runs
                    .iter()
                    .map(move |run| line.left + run.x + run.width)
            })
            .fold(0.0_f32, f32::max);
        assert!(rightmost <= 400.0 + 0.5, "はみ出している: {rightmost}");
    }

    /// これ以上縮められないときは最小幅のまま（横スクロールで見せる）。
    #[test]
    fn unshrinkable_table_keeps_minimum_widths() {
        let text = "| aaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbb |
|---|---|
| x | y |
";
        // 最小幅の合計より狭い幅を与える
        let result = layout_first(text, 40.0);
        assert!(!result.lines.is_empty(), "潰れて消えてはいけない");
    }

    /// 右寄せの列は左端が右へずれる（§3.3）。
    #[test]
    fn right_aligned_column_is_pushed_right() {
        // 見出しが本体より広い列でないと、寄せる余地が生まれない
        let left = layout_first(
            "| aaaa |
|:---|
| x |
",
            800.0,
        );
        let right = layout_first(
            "| aaaa |
|---:|
| x |
",
            800.0,
        );
        assert!(
            right.lines[1].left > left.lines[1].left,
            "{} > {}",
            right.lines[1].left,
            left.lines[1].left
        );
    }

    /// 背の高いセルに行の高さを合わせる。
    #[test]
    fn row_height_follows_the_tallest_cell() {
        let text = format!(
            "| a | b |
|---|---|
| x | {} |
",
            "い".repeat(40)
        );
        let short = layout_first(
            "| a | b |
|---|---|
| x | y |
",
            300.0,
        )
        .height;
        let tall = layout_first(&text, 300.0).height;
        assert!(tall > short, "{tall} > {short}");
    }

    /// 状態を差し替えられる引き口（試験用）。
    struct FakeEmbeds(Option<JobState>);

    impl EmbedLookup for FakeEmbeds {
        fn state(&self, _key: &EmbedKey) -> Option<JobState> {
            self.0.clone()
        }
    }

    fn layout_with_embed(text: &str, state: Option<JobState>) -> LaidOutBlock {
        let measurer = FixedMeasurer::default();
        let lookup = FakeEmbeds(state);
        let cx = LayoutContext {
            width: 800.0,
            measurer: &measurer,
            block_spacing: 12.0,
            indent_unit: 24.0,
            base_dir: None,
            embeds: Some(&lookup),
        };
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        layout_block(block, &text[block.bytes.clone()], &cx)
    }

    const MERMAID: &str = "```mermaid
graph TD; A-->B
```
";

    /// mermaid のフェンスは**コードではなく図**として扱う。
    #[test]
    fn mermaid_fence_is_an_embed() {
        let blocks = scan_lines(MERMAID).blocks;
        let block = &blocks[0];
        let request = embed_source(block, &MERMAID[block.bytes.clone()], 800.0, None)
            .expect("図として認識される");
        assert_eq!(request.kind, EmbedKind::Diagram);
        // フェンス行を含まない
        assert_eq!(request.text.trim(), "graph TD; A-->B");
    }

    /// **`<img>` タグ 1 つの段落も箱にする**（v2.1.0 R-17）。
    ///
    /// 安い判定（`is_embed_block`）と本当の判定（`embed_source`）の**両方**が
    /// 認めないと、描く依頼が出ずに「描画中」のまま止まる
    #[test]
    fn a_lone_img_tag_is_an_embed_with_its_width() {
        let text = "<img width=\"200\" alt=\"image\" src=\"img/a.png\">\n";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        let source = &text[block.bytes.clone()];
        assert!(is_embed_block(block, source), "安い判定で落ちている");
        let request = embed_source(block, source, 800.0, None).expect("画像として認識される");
        assert_eq!(request.kind, EmbedKind::Image);
        assert_eq!(request.width, 200.0, "幅の指定が効いていない");
        // 画面より広い指定は画面の幅で止める
        let wide = "<img width=\"2000\" src=\"img/a.png\">\n";
        let blocks = scan_lines(wide).blocks;
        let request = embed_source(&blocks[0], wide, 800.0, None).expect("画像");
        assert_eq!(request.width, 800.0);
    }

    /// **画像だけの段落は箱にする**（§16.12）。
    #[test]
    fn lone_image_paragraph_is_an_embed() {
        let text = "![図](img/a.png)\n";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        let base = std::path::Path::new("C:/docs");
        let request = embed_source(block, &text[block.bytes.clone()], 800.0, Some(base))
            .expect("画像として認識される");
        assert_eq!(request.kind, EmbedKind::Image);
        // 相対パスが文書の位置を基準に解決されている
        assert!(request
            .text
            .replace('\\', "/")
            .ends_with("C:/docs/img/a.png"));
    }

    /// **ネットワークの参照は埋め込みにしない**（取りに行かない）。
    #[test]
    fn remote_image_is_not_an_embed() {
        let text = "![図](https://example.com/a.png)\n";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        assert!(embed_source(block, &text[block.bytes.clone()], 800.0, None).is_none());
    }

    /// 文章が続く画像は段落のまま（行の中に箱を置く話になるため。DD-OPEN-13）。
    #[test]
    fn image_with_text_stays_a_paragraph() {
        let text = "![図](a.png) と書いた。\n";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        assert!(embed_source(block, &text[block.bytes.clone()], 800.0, None).is_none());

        let result = layout_with_embed(text, None);
        assert!(result.embed.is_none());
        assert!(
            rendered(&result).contains("と書いた"),
            "{}",
            rendered(&result)
        );
    }

    /// 判定は安く済ませる（キャッシュを引く前に呼ばれるため）。
    #[test]
    fn embed_detection_does_not_need_the_body() {
        let text = "![図](a.png)\n";
        let blocks = scan_lines(text).blocks;
        assert!(is_embed_block(&blocks[0], text));

        let plain = "ふつうの段落\n";
        let blocks = scan_lines(plain).blocks;
        assert!(!is_embed_block(&blocks[0], plain));
    }

    /// **erDiagram は図にしない**（OPEN-210）。ラベルが欠けるため原文を見せる。
    #[test]
    fn er_diagram_falls_back_to_code() {
        let text = "```mermaid
erDiagram
    顧客 ||--o{ 注文 : 持つ
```
";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        assert!(embed_source(block, &text[block.bytes.clone()], 800.0, None).is_none());

        // コードブロックとして原文が出る
        let result = layout_with_embed(text, None);
        assert!(result.embed.is_none());
        assert!(
            rendered(&result).contains("erDiagram"),
            "{}",
            rendered(&result)
        );
    }

    #[test]
    fn math_fence_is_an_embed() {
        let text = "```math
x^2
```
";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        let request = embed_source(block, &text[block.bytes.clone()], 800.0, None).expect("数式");
        assert_eq!(request.kind, EmbedKind::Math);
    }

    /// ふつうのコードは埋め込みではない。
    #[test]
    fn ordinary_code_is_not_an_embed() {
        let text = "```rust
fn main() {}
```
";
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        assert!(embed_source(block, &text[block.bytes.clone()], 800.0, None).is_none());
    }

    /// 結果が届くまでは**寸法が確定したプレースホルダ**を置く（§16.5）。
    #[test]
    fn pending_embed_reserves_the_placeholder_height() {
        let result = layout_with_embed(MERMAID, Some(JobState::Pending));
        let placement = result.embed.expect("箱が置かれる");
        assert!(
            (placement.top + placement.height - PLACEHOLDER_HEIGHT).abs() < 0.01,
            "{placement:?}"
        );
        assert!(
            rendered(&result).contains("描画中"),
            "{}",
            rendered(&result)
        );
    }

    /// まだ依頼していなくても、待ちとして同じ高さを取る。
    #[test]
    fn unrequested_embed_also_reserves_height() {
        let result = layout_with_embed(MERMAID, None);
        assert!(result.embed.is_some());
    }

    /// 結果が届いたら実寸へ置き換わる。
    #[test]
    fn finished_embed_uses_its_real_size() {
        let embed = crate::embed::RenderedEmbed {
            width: 400,
            height: 300,
            pixels: std::sync::Arc::new(Vec::new()),
        };
        let result = layout_with_embed(MERMAID, Some(JobState::Done(embed)));
        let placement = result.embed.expect("箱が置かれる");
        assert_eq!(placement.height, 300.0);
        assert!(result.lines.is_empty(), "待ちの文言は消える");
    }

    /// **失敗は画面に出す。** 黙って空白にしない。
    #[test]
    fn failed_embed_shows_the_reason() {
        let error = crate::embed::EmbedError::Failed("構文誤り".to_owned());
        let result = layout_with_embed(MERMAID, Some(JobState::Failed(error)));
        assert!(result.embed.is_none(), "箱は取らない");
        let text = rendered(&result);
        assert!(text.contains("構文誤り"), "{text}");
    }

    /// 埋め込みは地を敷かない（図の背景と重なるため）。
    #[test]
    fn embed_has_no_code_background() {
        let result = layout_with_embed(MERMAID, Some(JobState::Pending));
        assert!(!result.code_background);
    }

    #[test]
    fn empty_block_has_height() {
        let measurer = FixedMeasurer::default();
        let cx = context(800.0, &measurer);
        let block = Block {
            bytes: 0..0,
            start_line: 0,
            line_count: 0,
            kind: BlockKind::Paragraph,
            revision: 0,
        };
        let result = layout_block(&block, "", &cx);
        assert!(result.height > 0.0);
    }
}
