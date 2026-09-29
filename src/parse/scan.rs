//! 行走査（§12.6）。
//!
//! 行頭とコードフェンスの内外だけを見てブロック境界を決める。
//! 文法解析はしない。表やリストの入れ子までは追わない。
//!
//! **10MB でも 5〜16ms で終わることが、増分解析ライブラリを使わない判断の根拠**
//! である（§8.3）。編集でフェンスの開閉が変わった場合に全走査へ
//! 落とせるのは、この速さがあってこそである。

use std::ops::Range;

/// ブロックの種別。行走査で分かる粒度に留める。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    /// `#` の数（1〜6）
    Heading(u8),
    /// フェンスで囲まれたコード。言語指定があれば持つ
    Code {
        language: Option<String>,
    },
    /// `|` で始まる行の塊
    Table,
    /// `-` `*` `+` または `1.` で始まる行の塊
    List,
    /// `>` で始まる行の塊
    Quote,
    /// `---` `***` `___`
    Rule,
    /// `<!-- pagebreak -->`（PDF の明示的な改ページ。§5.5）
    PageBreak,
}

/// 1 ブロック。
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub bytes: Range<usize>,
    pub start_line: usize,
    /// このブロックが占める行数
    pub line_count: usize,
    pub kind: BlockKind,
    /// **このブロックの一意な番号。** レイアウトキャッシュの鍵に使う（§3.8）。
    ///
    /// 走査で作られた時点で `Document` が採番する。位置がずれただけのブロックは
    /// 番号を引き継ぐので、キャッシュが効き続ける。
    ///
    /// **添字を鍵にしてはいけない。** ブロックが削除されると、別の内容が同じ添字を
    /// 取り、古い結果を引いてしまう。
    pub revision: u64,
}

impl Block {
    /// 見出しなら見出しレベルを返す。
    pub fn heading_level(&self) -> Option<u8> {
        match self.kind {
            BlockKind::Heading(level) => Some(level),
            _ => None,
        }
    }
}

/// 走査結果。
#[derive(Debug, Default)]
pub struct ScanResult {
    pub blocks: Vec<Block>,
    /// 文書全体の行数
    pub line_count: usize,
    /// **フェンスが閉じずに終わったか。** 編集時の再解析範囲の判断に使う
    /// （§12.8）
    pub unterminated_fence: bool,
}

/// フェンスの開始マーカー。
///
/// 開始時の文字（``` か ~~~）と長さを覚えておき、**一致するものだけを終了とみなす**。
/// これをしないと、``` の中に ~~~ が現れる文書で境界を誤る。
#[derive(Debug, Clone, Copy)]
struct Fence {
    marker: u8,
    length: usize,
}

/// 差分走査の結果（§12.8）。
#[derive(Debug, Default)]
pub struct Rescan {
    pub blocks: Vec<Block>,
    /// **旧ブロックと再同期した絶対バイト位置。**
    /// `Some` なら、ここ以降の旧ブロックはバイト位置をずらすだけでよい。
    /// `None` は末尾まで走査したことを表す（フェンスの開閉が変わった場合など）
    pub resync_at: Option<usize>,
    pub unterminated_fence: bool,
}

/// 安全な再開点から走査し、旧ブロックと再同期したら打ち切る。
///
/// これが**増分解析ライブラリを使わない**判断（DEC-204）の実体である。
/// フェンスの開閉が変わらなければ影響範囲だけで済み、変わった場合は末尾まで
/// 走査し直す。後者でも 10MB で 5〜16ms なので許容できる。
///
/// `slice` は `from_byte` から始まる本文の一部。`is_sync_point` には
/// **編集後の位置へずらした**旧ブロックの開始位置を渡す。
pub fn rescan_from(
    slice: &str,
    from_byte: usize,
    from_line: usize,
    is_sync_point: &dyn Fn(usize) -> bool,
) -> Rescan {
    let mut scanner = Scanner::new(from_byte, from_line);
    // 再開点はブロックの先頭なので、直前は空行だったものとして始める
    scanner.previous_blank = true;

    for line in split_lines(slice) {
        // ブロックの切れ目にいて、フェンスの外で、旧ブロックの開始位置と
        // 一致したら、以降は編集前と同じ構造である
        if scanner.current.is_none()
            && scanner.fence.is_none()
            && scanner.offset > from_byte
            && is_sync_point(scanner.offset)
        {
            return Rescan {
                blocks: scanner.blocks,
                resync_at: Some(scanner.offset),
                unterminated_fence: false,
            };
        }
        scanner.feed(line);
    }

    scanner.finish();
    Rescan {
        blocks: scanner.blocks,
        resync_at: None,
        unterminated_fence: scanner.fence.is_some(),
    }
}

/// 走査の途中状態。`scan_lines` と `rescan_from` で共有する。
struct Scanner {
    blocks: Vec<Block>,
    fence: Option<Fence>,
    previous_blank: bool,
    /// 構築中のブロック。(開始バイト, 開始行, 種別, 行数)
    current: Option<(usize, usize, BlockKind, usize)>,
    offset: usize,
    line_index: usize,
}

impl Scanner {
    fn new(offset: usize, line_index: usize) -> Self {
        Self {
            blocks: Vec::new(),
            fence: None,
            previous_blank: true,
            current: None,
            offset,
            line_index,
        }
    }

    fn feed(&mut self, line: &str) {
        let line_start = self.offset;
        self.offset += line.len();
        let body = strip_newline(line);
        let trimmed = body.trim_start();
        let indent = body.len() - trimmed.len();

        // --- フェンスの判定 ---
        // 行頭のインデントが 4 未満のときだけフェンスとして扱う（CommonMark）
        if indent < 4 {
            if let Some(open) = self.fence {
                if is_closing_fence(trimmed, open) {
                    // 閉じフェンスもブロックに含める
                    if let Some(block) = self.current.as_mut() {
                        block.3 += 1;
                    }
                    let end = self.offset;
                    finish(&mut self.current, end, &mut self.blocks);
                    self.fence = None;
                    self.previous_blank = false;
                    self.line_index += 1;
                    return;
                }
            } else if let Some(open) = opening_fence(trimmed) {
                finish(&mut self.current, line_start, &mut self.blocks);
                let language = fence_language(trimmed, open.length);
                self.current = Some((line_start, self.line_index, BlockKind::Code { language }, 1));
                self.fence = Some(open);
                self.previous_blank = false;
                self.line_index += 1;
                return;
            }
        }

        // --- フェンスの中は読み飛ばす ---
        if self.fence.is_some() {
            if let Some(block) = self.current.as_mut() {
                block.3 += 1;
            }
            self.line_index += 1;
            return;
        }

        // --- 空行 ---
        if trimmed.is_empty() {
            finish(&mut self.current, line_start, &mut self.blocks);
            self.previous_blank = true;
            self.line_index += 1;
            return;
        }

        let kind = classify(trimmed);

        // 見出し・罫線・改ページは常に単独のブロックにする
        let standalone = matches!(
            kind,
            BlockKind::Heading(_) | BlockKind::Rule | BlockKind::PageBreak
        );

        if self.previous_blank || standalone || self.current.is_none() {
            finish(&mut self.current, line_start, &mut self.blocks);
            self.current = Some((line_start, self.line_index, kind, 1));
            if standalone {
                let end = self.offset;
                finish(&mut self.current, end, &mut self.blocks);
            }
        } else if let Some(block) = self.current.as_mut() {
            block.3 += 1;
        }

        self.previous_blank = false;
        self.line_index += 1;
    }

    fn finish(&mut self) {
        let end = self.offset;
        finish(&mut self.current, end, &mut self.blocks);
    }
}

/// 本文を走査してブロックの列を得る。
pub fn scan_lines(text: &str) -> ScanResult {
    let mut scanner = Scanner::new(0, 0);
    for line in split_lines(text) {
        scanner.feed(line);
    }
    scanner.finish();

    ScanResult {
        line_count: scanner.line_index.max(1),
        unterminated_fence: scanner.fence.is_some(),
        blocks: scanner.blocks,
    }
}

/// 構築中のブロックを確定させる。
fn finish(
    current: &mut Option<(usize, usize, BlockKind, usize)>,
    end: usize,
    blocks: &mut Vec<Block>,
) {
    if let Some((start, start_line, kind, line_count)) = current.take() {
        if end > start {
            blocks.push(Block {
                bytes: start..end,
                start_line,
                line_count,
                kind,
                // 採番は Document が行う（走査は番号の連続性を知らない）
                revision: 0,
            });
        }
    }
}

/// 改行を含めて 1 行ずつ返す（`str::lines` は改行を落とすためバイト位置がずれる）。
fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        match rest.find('\n') {
            Some(index) => {
                let (line, tail) = rest.split_at(index + 1);
                rest = tail;
                Some(line)
            }
            None => {
                let line = rest;
                rest = "";
                Some(line)
            }
        }
    })
}

/// 行末の改行を落とす。CRLF にも対応する（SCAN-04）。
fn strip_newline(line: &str) -> &str {
    line.strip_suffix('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or(line)
}

/// フェンスの開始なら、その情報を返す。
fn opening_fence(trimmed: &str) -> Option<Fence> {
    for marker in *b"`~" {
        let count = trimmed.bytes().take_while(|byte| *byte == marker).count();
        if count >= 3 {
            // ``` の情報文字列にバッククォートを含めてはいけない（CommonMark）
            if marker == b'`' && trimmed[count..].contains('`') {
                return None;
            }
            return Some(Fence {
                marker,
                length: count,
            });
        }
    }
    None
}

/// 閉じフェンスかどうか。**開始と同じ文字で、同じ長さ以上**であることを要求する。
fn is_closing_fence(trimmed: &str, open: Fence) -> bool {
    let count = trimmed
        .bytes()
        .take_while(|byte| *byte == open.marker)
        .count();
    count >= open.length && trimmed[count..].trim().is_empty()
}

/// フェンスの情報文字列から言語名を取り出す。
fn fence_language(trimmed: &str, marker_length: usize) -> Option<String> {
    let info = trimmed[marker_length..].trim();
    let word = info.split_whitespace().next()?;
    if word.is_empty() {
        None
    } else {
        Some(word.to_ascii_lowercase())
    }
}

/// 行頭の記号からブロック種別を決める。
fn classify(trimmed: &str) -> BlockKind {
    if let Some(level) = atx_heading_level(trimmed) {
        return BlockKind::Heading(level);
    }
    if is_thematic_break(trimmed) {
        return BlockKind::Rule;
    }
    if trimmed.starts_with("<!-- pagebreak -->") {
        return BlockKind::PageBreak;
    }
    if trimmed.starts_with('>') {
        return BlockKind::Quote;
    }
    if trimmed.starts_with('|') {
        return BlockKind::Table;
    }
    if is_list_marker(trimmed) {
        return BlockKind::List;
    }
    BlockKind::Paragraph
}

/// ATX 見出しのレベル。
///
/// **`#` の直後に空白が要る**（CommonMark）。`#見出し` は見出しにしない（SCAN-07）。
fn atx_heading_level(trimmed: &str) -> Option<u8> {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &trimmed[hashes..];
    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
        Some(hashes as u8)
    } else {
        None
    }
}

/// `---` `***` `___`（3 文字以上、ほかに空白以外を含まない）。
fn is_thematic_break(trimmed: &str) -> bool {
    for marker in ['-', '*', '_'] {
        let count = trimmed.chars().filter(|c| *c == marker).count();
        let others = trimmed
            .chars()
            .filter(|c| *c != marker && !c.is_whitespace())
            .count();
        if count >= 3 && others == 0 {
            return true;
        }
    }
    false
}

/// 箇条書き（`- ` `* ` `+ `）または番号付き（`1. ` `1) `）。
fn is_list_marker(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    match chars.next() {
        Some('-') | Some('*') | Some('+') => {
            matches!(chars.next(), Some(' ') | Some('\t') | None)
        }
        Some(first) if first.is_ascii_digit() => {
            let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
            // 番号は 9 桁まで（CommonMark）
            if digits > 9 {
                return false;
            }
            let rest = &trimmed[digits..];
            (rest.starts_with(". ") || rest.starts_with(") ")) || rest == "." || rest == ")"
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<BlockKind> {
        scan_lines(text)
            .blocks
            .into_iter()
            .map(|b| b.kind)
            .collect()
    }

    /// SCAN-01: ``` で開き ``` で閉じる → フェンス内が 1 ブロック
    #[test]
    fn fence_opens_and_closes() {
        let text = "前の段落\n\n```rust\nfn main() {}\n\nlet x = 1;\n```\n\n後の段落\n";
        let blocks = scan_lines(text).blocks;
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(
            blocks[1].kind,
            BlockKind::Code {
                language: Some("rust".to_owned())
            }
        );
        // フェンス内の空行でブロックが割れないこと
        assert_eq!(blocks[1].line_count, 5);
        assert_eq!(blocks[2].kind, BlockKind::Paragraph);
    }

    /// SCAN-02: ``` で開き ~~~ が現れても閉じない（マーカー不一致）
    #[test]
    fn fence_marker_must_match() {
        let text = "```\nコード\n~~~\nまだコード\n```\n";
        let result = scan_lines(text);
        assert_eq!(result.blocks.len(), 1);
        assert!(!result.unterminated_fence);
        assert_eq!(result.blocks[0].line_count, 5);
    }

    /// SCAN-03: フェンスが閉じないまま EOF → 末尾までフェンス内
    #[test]
    fn unterminated_fence_reaches_eof() {
        let text = "段落\n\n```\nコード\nまだコード\n";
        let result = scan_lines(text);
        assert!(result.unterminated_fence);
        assert_eq!(result.blocks.len(), 2);
        assert!(matches!(result.blocks[1].kind, BlockKind::Code { .. }));
    }

    /// SCAN-04: CRLF 改行でも LF と同じ分割になる
    #[test]
    fn crlf_is_equivalent_to_lf() {
        let lf = scan_lines("# 見出し\n\n段落です\n");
        let crlf = scan_lines("# 見出し\r\n\r\n段落です\r\n");
        let lf_kinds: Vec<_> = lf.blocks.iter().map(|b| b.kind.clone()).collect();
        let crlf_kinds: Vec<_> = crlf.blocks.iter().map(|b| b.kind.clone()).collect();
        assert_eq!(lf_kinds, crlf_kinds);
        assert_eq!(lf.blocks.len(), 2);
    }

    /// SCAN-06: 空行が連続しても空ブロックを作らない
    #[test]
    fn blank_lines_do_not_create_blocks() {
        let text = "段落 A\n\n\n\n\n段落 B\n";
        let blocks = scan_lines(text).blocks;
        assert_eq!(blocks.len(), 2);
    }

    /// SCAN-07: `#見出し`（空白なし）は見出しにしない
    #[test]
    fn hash_without_space_is_not_heading() {
        assert_eq!(kinds("#見出し\n"), vec![BlockKind::Paragraph]);
        assert_eq!(kinds("# 見出し\n"), vec![BlockKind::Heading(1)]);
        assert_eq!(kinds("###### 6 階層\n"), vec![BlockKind::Heading(6)]);
        // 7 個は見出しではない
        assert_eq!(kinds("####### 7 階層\n"), vec![BlockKind::Paragraph]);
    }

    /// SCAN-08: `<!-- pagebreak -->` を PageBreak として拾う
    #[test]
    fn pagebreak_is_detected() {
        let blocks = scan_lines("段落\n\n<!-- pagebreak -->\n\n次の段落\n").blocks;
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].kind, BlockKind::PageBreak);
    }

    /// 見出しは空行が無くても単独のブロックになる
    #[test]
    fn heading_breaks_without_blank_line() {
        let blocks = scan_lines("段落\n# 見出し\n続きの段落\n").blocks;
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].kind, BlockKind::Heading(1));
    }

    #[test]
    fn classifies_common_blocks() {
        assert_eq!(kinds("| a | b |\n"), vec![BlockKind::Table]);
        assert_eq!(kinds("> 引用\n"), vec![BlockKind::Quote]);
        assert_eq!(kinds("- 箇条書き\n"), vec![BlockKind::List]);
        assert_eq!(kinds("1. 番号付き\n"), vec![BlockKind::List]);
        assert_eq!(kinds("---\n"), vec![BlockKind::Rule]);
        // 区切り線に見えるが文字が混ざる場合は段落
        assert_eq!(kinds("--- x\n"), vec![BlockKind::Paragraph]);
    }

    /// バイト範囲が本文と対応していること（レイアウトが原文を切り出すために要る）
    #[test]
    fn byte_ranges_cover_source() {
        let text = "# 見出し\n\n本文です。\n\n- 項目\n";
        let blocks = scan_lines(text).blocks;
        for block in &blocks {
            let slice = &text[block.bytes.clone()];
            assert!(!slice.trim().is_empty(), "空のブロックがある: {block:?}");
        }
        // 先頭ブロックは見出しの行そのもの
        assert_eq!(&text[blocks[0].bytes.clone()].trim_end(), &"# 見出し");
    }

    /// 日本語を含む文書でバイト境界を壊さない
    #[test]
    fn handles_multibyte_text() {
        let text = "# 日本語の見出し\n\nこれは日本語の段落である。全角と ASCII の混在。\n";
        let blocks = scan_lines(text).blocks;
        assert_eq!(blocks.len(), 2);
        for block in &blocks {
            // 範囲が文字境界に乗っていること（乗っていなければここで panic する）
            let _ = &text[block.bytes.clone()];
        }
    }
}
