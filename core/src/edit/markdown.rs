//! Markdown の書き方を助ける道具（v2.1.0 R-06 / R-14 / R-15 / R-16）。
//!
//! **ここも文書と画面を知らない。** `&str` を受けて、どう書き換えるかを
//! 返すだけである（§4.2）。呼び出し側は、結果を 1 つの編集として積む。

use crate::layout::is_wide;

// ---------------------------------------------------------------------------
// リストと引用の継続（R-14）
// ---------------------------------------------------------------------------

/// `Enter` を押したときにどうするか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enter {
    /// 改行のあとに続ける記号（`"\n- "` のように改行を含めて返す）
    Continue(String),
    /// 中身の無い項目で押した。**行の頭から `caret` までを `with` に置き換えて**
    /// リストを抜ける（改行は入れない）
    Exit { with: String },
}

/// 行頭の記号を読み解いたもの。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prefix {
    /// 引用の外側の字下げ
    indent: String,
    /// 引用の記号（`> ` を段の数だけ）
    quotes: usize,
    /// 引用の内側の字下げ（引用の中のリスト）
    inner: String,
    marker: Option<Marker>,
    /// チェックボックスが付いているか
    task: bool,
    /// 中身の始まり（バイト）
    content: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Marker {
    Bullet(char),
    Ordered { number: u64, delimiter: char },
}

/// `---` `* * *` のような区切り線か。**リストの記号と見分ける**
fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let marks = trimmed.chars().filter(|ch| *ch == first).count();
    marks >= 3
        && trimmed
            .chars()
            .all(|ch| ch == first || ch == ' ' || ch == '\t')
}

fn leading_whitespace(text: &str) -> &str {
    let end = text
        .find(|ch: char| ch != ' ' && ch != '\t')
        .unwrap_or(text.len());
    &text[..end]
}

fn parse_prefix(line: &str) -> Prefix {
    let indent = leading_whitespace(line).to_owned();
    let mut at = indent.len();

    // 引用（`>` の後ろの空白は 1 つまで記号に含める）
    let mut quotes = 0;
    loop {
        let rest = &line[at..];
        let skipped = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let after_space = &rest[skipped..];
        if quotes > 0 && after_space.starts_with('>') || quotes == 0 && rest.starts_with('>') {
            let start = if quotes > 0 { at + skipped } else { at };
            at = start + 1;
            if line[at..].starts_with(' ') {
                at += 1;
            }
            quotes += 1;
        } else {
            break;
        }
    }

    let inner = if quotes > 0 {
        leading_whitespace(&line[at..]).to_owned()
    } else {
        String::new()
    };
    at += inner.len();

    let rest = &line[at..];
    let mut marker = None;
    let mut chars = rest.char_indices();
    if let Some((_, first)) = chars.next() {
        if matches!(first, '-' | '*' | '+') {
            let next = rest[1..].chars().next();
            // **`-` だけの行は記号とみなさない。** 見出しの下線（setext）を
            // 引いて `Enter` を押したら、行ごと消えてしまう
            if next == Some(' ') || next == Some('\t') {
                marker = Some(Marker::Bullet(first));
                at += 2;
            }
        } else if first.is_ascii_digit() {
            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            let after = &rest[digits..];
            let delimiter = after.chars().next();
            if digits <= 9 && matches!(delimiter, Some('.') | Some(')')) {
                let space = after[1..].chars().next();
                if space == Some(' ') || space == Some('\t') {
                    let number = rest[..digits].parse::<u64>().unwrap_or(1);
                    marker = Some(Marker::Ordered {
                        number,
                        delimiter: delimiter.unwrap_or('.'),
                    });
                    at += digits + 2;
                }
            }
        }
    }

    let mut task = false;
    if marker.is_some() {
        let rest = &line[at..];
        for box_ in ["[ ]", "[x]", "[X]"] {
            if let Some(after) = rest.strip_prefix(box_) {
                if after.is_empty() || after.starts_with(' ') {
                    task = true;
                    at += box_.len() + usize::from(after.starts_with(' '));
                    break;
                }
            }
        }
    }

    Prefix {
        indent,
        quotes,
        inner,
        marker,
        task,
        content: at,
    }
}

/// キャレットの行で `Enter` を押したときの振る舞い（R-14）。
///
/// * `line` — キャレットのある行（改行を含まない）
/// * `caret` — 行の中のキャレットの位置（バイト）
///
/// リストでも引用でもなければ `None`（ふつうの改行）。
pub fn enter(line: &str, caret: usize) -> Option<Enter> {
    if !line.is_char_boundary(caret.min(line.len())) || is_rule(line) {
        return None;
    }
    let caret = caret.min(line.len());
    let prefix = parse_prefix(line);
    if prefix.quotes == 0 && prefix.marker.is_none() {
        return None;
    }
    // **記号の途中で押したら、ふつうの改行。** 記号を割ることになる
    if caret < prefix.content {
        return None;
    }

    let quotes = "> ".repeat(prefix.quotes);
    let empty = line[prefix.content..].trim().is_empty() && line[caret..].trim().is_empty();

    if empty {
        // **中身の無い項目で押したら、記号を消して抜ける**（VS Code と同じ）
        let with = match &prefix.marker {
            Some(_) => format!("{}{}", prefix.indent, quotes.trim_end()),
            // 引用だけなら 1 段だけ浅くする
            None => format!(
                "{}{}",
                prefix.indent,
                "> ".repeat(prefix.quotes.saturating_sub(1)).trim_end()
            ),
        };
        return Some(Enter::Exit { with });
    }

    let marker = match &prefix.marker {
        Some(Marker::Bullet(ch)) => format!("{ch} "),
        Some(Marker::Ordered { number, delimiter }) => {
            format!("{}{delimiter} ", number.saturating_add(1))
        }
        None => String::new(),
    };
    let task = if prefix.task { "[ ] " } else { "" };
    Some(Enter::Continue(format!(
        "\n{}{}{}{}{}",
        prefix.indent, quotes, prefix.inner, marker, task
    )))
}

// ---------------------------------------------------------------------------
// コメント（R-06）
// ---------------------------------------------------------------------------

/// 言語ごとのコメントの書き方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommentSyntax {
    pub line: Option<&'static str>,
    pub block: Option<(&'static str, &'static str)>,
}

/// Markdown 本文（と HTML）のコメント。**行コメントは無い**
pub const MARKDOWN_COMMENT: CommentSyntax = CommentSyntax {
    line: None,
    block: Some(("<!--", "-->")),
};

/// コードフェンスの言語名から、コメントの書き方を引く。
///
/// **知らない言語は `None`。** 呼び出し側は Markdown の書き方へ戻す
pub fn comment_syntax(language: &str) -> Option<CommentSyntax> {
    let c_like = CommentSyntax {
        line: Some("//"),
        block: Some(("/*", "*/")),
    };
    let hash = CommentSyntax {
        line: Some("#"),
        block: None,
    };
    let language = language.trim().to_ascii_lowercase();
    // `rust,ignore` のような付記は落とす
    let language = language.split([',', ' ', '{']).next().unwrap_or_default();
    Some(match language {
        "rust" | "rs" | "c" | "h" | "cpp" | "c++" | "cc" | "hpp" | "cs" | "csharp" | "c#"
        | "java" | "javascript" | "js" | "jsx" | "mjs" | "typescript" | "ts" | "tsx" | "go"
        | "golang" | "swift" | "kotlin" | "kt" | "scala" | "dart" | "php" | "groovy" | "gradle"
        | "objc" | "objective-c" | "zig" | "v" | "json5" | "jsonc" | "proto" | "protobuf"
        | "glsl" | "hlsl" | "wgsl" | "solidity" => c_like,
        "python" | "py" | "ruby" | "rb" | "sh" | "bash" | "zsh" | "fish" | "shell" | "console"
        | "yaml" | "yml" | "toml" | "perl" | "pl" | "r" | "dockerfile" | "docker" | "makefile"
        | "make" | "cmake" | "nim" | "elixir" | "ex" | "exs" | "crystal" | "julia" | "jl"
        | "tcl" | "conf" | "nginx" | "gitignore" | "properties" => hash,
        "powershell" | "ps1" | "pwsh" | "ps" => CommentSyntax {
            line: Some("#"),
            block: Some(("<#", "#>")),
        },
        "sql" | "mysql" | "postgresql" | "postgres" | "plsql" | "tsql" | "sqlite" => {
            CommentSyntax {
                line: Some("--"),
                block: Some(("/*", "*/")),
            }
        }
        "lua" => CommentSyntax {
            line: Some("--"),
            block: Some(("--[[", "]]")),
        },
        "haskell" | "hs" | "elm" | "purescript" => CommentSyntax {
            line: Some("--"),
            block: Some(("{-", "-}")),
        },
        "ada" | "vhdl" => CommentSyntax {
            line: Some("--"),
            block: None,
        },
        "html" | "htm" | "xml" | "svg" | "xhtml" | "vue" | "markdown" | "md" => MARKDOWN_COMMENT,
        "css" => CommentSyntax {
            line: None,
            block: Some(("/*", "*/")),
        },
        "scss" | "sass" | "less" => c_like,
        "lisp" | "clojure" | "clj" | "scheme" | "racket" | "elisp" | "emacs-lisp" | "ini" => {
            CommentSyntax {
                line: Some(";"),
                block: None,
            }
        }
        "vb" | "vbnet" | "vba" | "vbscript" => CommentSyntax {
            line: Some("'"),
            block: None,
        },
        "bat" | "batch" | "cmd" => CommentSyntax {
            line: Some("REM"),
            block: None,
        },
        "tex" | "latex" | "matlab" | "erlang" | "erl" | "prolog" => CommentSyntax {
            line: Some("%"),
            block: None,
        },
        "ocaml" | "ml" | "fsharp" | "fs" | "pascal" | "delphi" => CommentSyntax {
            line: if matches!(language, "fsharp" | "fs") {
                Some("//")
            } else {
                None
            },
            block: Some(("(*", "*)")),
        },
        "vim" | "viml" => CommentSyntax {
            line: Some("\""),
            block: None,
        },
        _ => return None,
    })
}

/// 行コメントを付ける／外す（R-06）。
///
/// `text` は**行の頭から行の終わりまで**（複数行可。最後の改行はあってもよい）。
/// **空行は数えない。** 空行を含めて「すべてコメントか」を見ると、
/// 段落の間の空行のせいで外せなくなる
pub fn toggle_line_comment(text: &str, syntax: CommentSyntax) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let blank = |line: &str| line.trim().is_empty();

    let Some(token) = syntax.line else {
        return toggle_wrapped_lines(&lines, syntax.block.unwrap_or(("<!--", "-->")));
    };

    let commented = |line: &str| line.trim_start().starts_with(token);
    let all =
        lines.iter().filter(|l| !blank(l)).all(|l| commented(l)) && lines.iter().any(|l| !blank(l));

    if all {
        return lines
            .iter()
            .map(|line| {
                if blank(line) {
                    return (*line).to_owned();
                }
                let indent = leading_whitespace(line);
                let rest = &line[indent.len() + token.len()..];
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                format!("{indent}{rest}")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    // **いちばん浅い字下げの位置に揃えて付ける**（VS Code と同じ）。
    // 行ごとの字下げに付けると、コメントの記号がガタガタに並ぶ
    let depth = lines
        .iter()
        .filter(|l| !blank(l))
        .map(|l| leading_whitespace(l).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| {
            if blank(line) {
                return (*line).to_owned();
            }
            let at = depth.min(leading_whitespace(line).len());
            format!("{}{token} {}", &line[..at], &line[at..])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 行ごとに `<!-- … -->` で包む／外す（Markdown の「行コメント」）。
fn toggle_wrapped_lines(lines: &[&str], (open, close): (&str, &str)) -> String {
    let blank = |line: &str| line.trim().is_empty();
    let wrapped = |line: &str| {
        let trimmed = line.trim();
        trimmed.starts_with(open)
            && trimmed.ends_with(close)
            && trimmed.len() >= open.len() + close.len()
    };
    let all =
        lines.iter().filter(|l| !blank(l)).all(|l| wrapped(l)) && lines.iter().any(|l| !blank(l));

    lines
        .iter()
        .map(|line| {
            if blank(line) {
                return (*line).to_owned();
            }
            let indent = leading_whitespace(line);
            let body = line[indent.len()..].trim_end();
            if all {
                let inner = &body[open.len()..body.len() - close.len()];
                let inner = inner.strip_prefix(' ').unwrap_or(inner);
                let inner = inner.strip_suffix(' ').unwrap_or(inner);
                format!("{indent}{inner}")
            } else {
                format!("{indent}{open} {body} {close}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// ブロックコメントを付ける／外す（R-06）。
///
/// **ブロックの書き方が無い言語では行コメントで代える**（要件定義書 R-06）。
pub fn toggle_block_comment(text: &str, syntax: CommentSyntax) -> String {
    let Some((open, close)) = syntax.block else {
        return toggle_line_comment(text, syntax);
    };

    // 最後の改行は外に出しておく（行選択は改行まで含む）
    let (body, tail) = match text.strip_suffix('\n') {
        Some(body) => (body, "\n"),
        None => (text, ""),
    };

    let trimmed = body.trim();
    if trimmed.starts_with(open)
        && trimmed.ends_with(close)
        && trimmed.len() >= open.len() + close.len()
    {
        let start = body.find(open).unwrap_or(0);
        let end = body.rfind(close).unwrap_or(body.len());
        let mut inner = &body[start + open.len()..end];
        // 付けたときに入れた空白か改行を 1 つだけ外す
        inner = inner
            .strip_prefix('\n')
            .or_else(|| inner.strip_prefix(' '))
            .unwrap_or(inner);
        inner = inner
            .strip_suffix('\n')
            .or_else(|| inner.strip_suffix(' '))
            .unwrap_or(inner);
        return format!(
            "{}{inner}{}{tail}",
            &body[..start],
            &body[end + close.len()..]
        );
    }

    if body.contains('\n') {
        format!("{open}\n{body}\n{close}{tail}")
    } else {
        format!("{open} {body} {close}{tail}")
    }
}

// ---------------------------------------------------------------------------
// 強調・コード（R-15）
// ---------------------------------------------------------------------------

/// 選んだ範囲を記号で包むか外すか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// 範囲の外側にある記号を外す（前後それぞれ記号の長さぶん）
    RemoveOutside,
    /// 範囲の内側の両端にある記号を外す
    RemoveInside,
    /// 包む
    Add,
}

/// 範囲の外側（`before` の末尾と `after` の先頭）に並ぶ記号の数。
fn run_of(text: &str, mark: char, from_end: bool) -> usize {
    if from_end {
        text.chars().rev().take_while(|ch| *ch == mark).count()
    } else {
        text.chars().take_while(|ch| *ch == mark).count()
    }
}

/// `*` の並び `n` 個の中に、`marker` の強調が掛かっているか。
///
/// **太字と斜体は同じ字を使う。** `***a***` は太字かつ斜体、
/// `**a**` は太字だけ、`*a*` は斜体だけである
fn has_emphasis(n: usize, marker: &str) -> bool {
    match marker.len() {
        1 => n == 1 || n >= 3,
        _ => n >= 2,
    }
}

/// 包むか外すかを決める（R-15）。
///
/// * `before` — 範囲の前（同じ行の頭から）
/// * `selected` — 選んだ範囲
/// * `after` — 範囲の後ろ（同じ行の終わりまで）
pub fn toggle_marker(before: &str, selected: &str, after: &str, marker: &str) -> Wrap {
    let Some(mark) = marker.chars().next() else {
        return Wrap::Add;
    };
    let emphasis = mark == '*' || mark == '_';

    let outside = if emphasis {
        let n = run_of(before, mark, true).min(run_of(after, mark, false));
        has_emphasis(n, marker)
    } else {
        before.ends_with(marker) && after.starts_with(marker)
    };
    if outside {
        return Wrap::RemoveOutside;
    }

    let inside = if emphasis {
        let n = run_of(selected, mark, false).min(run_of(selected, mark, true));
        selected.chars().count() > 2 * n.min(3) && has_emphasis(n, marker)
    } else {
        selected.len() >= 2 * marker.len()
            && selected.starts_with(marker)
            && selected.ends_with(marker)
    };
    if inside {
        return Wrap::RemoveInside;
    }
    Wrap::Add
}

/// URL らしいか（R-15 のリンク・R-17 の貼り付け）。
///
/// **空白を含まず、スキームで始まるもの**だけを URL とみなす。
/// `example.com` のような書き方はリンクにしない（ファイル名と区別できない）
pub fn looks_like_url(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return false;
    }
    ["http://", "https://", "ftp://", "mailto:", "file://"]
        .iter()
        .any(|scheme| text.len() > scheme.len() && text.to_ascii_lowercase().starts_with(scheme))
}

// ---------------------------------------------------------------------------
// 表の整形（R-16）
// ---------------------------------------------------------------------------

/// 列の寄せ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    None,
    Left,
    Center,
    Right,
}

/// 表の 1 行を枠ごとに切る。
///
/// **`\|` とコードの中の `|` では切らない。** 切ると、中身に `|` を含む
/// 表が壊れる
fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let body = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let body = match body.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => body,
    };

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_code = false;
    let mut escaped = false;
    for ch in body.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                current.push(ch);
                escaped = true;
            }
            '`' => {
                in_code = !in_code;
                current.push(ch);
            }
            '|' if !in_code => {
                cells.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    cells.push(current.trim().to_owned());
    cells
}

fn delimiter_align(cell: &str) -> Option<Align> {
    let cell = cell.trim();
    let left = cell.starts_with(':');
    let right = cell.ends_with(':');
    let dashes = cell.trim_matches(':');
    if dashes.is_empty() || !dashes.chars().all(|ch| ch == '-') {
        return None;
    }
    Some(match (left, right) {
        (true, true) => Align::Center,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        (false, false) => Align::None,
    })
}

/// 表示の幅（半角 1・全角 2）。
pub fn display_width(text: &str) -> usize {
    text.chars().map(|ch| if is_wide(ch) { 2 } else { 1 }).sum()
}

fn pad(cell: &str, width: usize, align: Align) -> String {
    let gap = width.saturating_sub(display_width(cell));
    match align {
        Align::Right => format!("{}{cell}", " ".repeat(gap)),
        Align::Center => {
            let left = gap / 2;
            format!("{}{cell}{}", " ".repeat(left), " ".repeat(gap - left))
        }
        Align::None | Align::Left => format!("{cell}{}", " ".repeat(gap)),
    }
}

/// 表を読み解いたもの。
struct Table {
    indent: String,
    header: Vec<String>,
    aligns: Vec<Align>,
    rows: Vec<Vec<String>>,
}

fn parse_table(lines: &[&str]) -> Option<Table> {
    if lines.len() < 2 {
        return None;
    }
    let header = split_row(lines[0]);
    let aligns: Vec<Align> = split_row(lines[1])
        .iter()
        .map(|cell| delimiter_align(cell))
        .collect::<Option<Vec<_>>>()?;
    let rows: Vec<Vec<String>> = lines[2..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| split_row(line))
        .collect();
    Some(Table {
        indent: leading_whitespace(lines[0]).to_owned(),
        header,
        aligns,
        rows,
    })
}

fn render_table(table: &Table) -> Vec<String> {
    let columns = table
        .rows
        .iter()
        .map(Vec::len)
        .chain([table.header.len(), table.aligns.len()])
        .max()
        .unwrap_or(0);
    let cell = |row: &[String], column: usize| row.get(column).cloned().unwrap_or_default();
    let align = |column: usize| table.aligns.get(column).copied().unwrap_or(Align::None);

    // **区切り行が読める最低の幅は 3**（`---`）
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            std::iter::once(&table.header)
                .chain(&table.rows)
                .map(|row| display_width(&cell(row, column)))
                .max()
                .unwrap_or(0)
                .max(3)
        })
        .collect();

    let line = |row: &[String]| {
        let cells: Vec<String> = (0..columns)
            .map(|column| pad(&cell(row, column), widths[column], align(column)))
            .collect();
        format!("{}| {} |", table.indent, cells.join(" | "))
    };

    let delimiter: Vec<String> = (0..columns)
        .map(|column| {
            let width = widths[column];
            match align(column) {
                Align::None => "-".repeat(width),
                Align::Left => format!(":{}", "-".repeat(width - 1)),
                Align::Right => format!("{}:", "-".repeat(width - 1)),
                Align::Center => format!(":{}:", "-".repeat(width - 2)),
            }
        })
        .collect();

    let mut out = vec![line(&table.header)];
    out.push(format!("{}| {} |", table.indent, delimiter.join(" | ")));
    out.extend(table.rows.iter().map(|row| line(row)));
    out
}

/// 表の列幅を揃える（R-16）。表として読めなければ `None`。
///
/// **全角の字は 2 桁として数える。** 等幅フォントで縦が揃う
pub fn format_table(lines: &[&str]) -> Option<Vec<String>> {
    parse_table(lines).map(|table| render_table(&table))
}

/// 表の末尾に空の行を足して整形する（R-16）。
pub fn table_add_row(lines: &[&str]) -> Option<Vec<String>> {
    let mut table = parse_table(lines)?;
    let columns = table.header.len().max(table.aligns.len());
    table.rows.push(vec![String::new(); columns]);
    Some(render_table(&table))
}

/// 表の右端に空の列を足して整形する（R-16）。
pub fn table_add_column(lines: &[&str]) -> Option<Vec<String>> {
    let mut table = parse_table(lines)?;
    let columns = table
        .rows
        .iter()
        .map(Vec::len)
        .chain([table.header.len(), table.aligns.len()])
        .max()
        .unwrap_or(0);
    table.header.resize(columns, String::new());
    table.header.push(String::new());
    table.aligns.resize(columns, Align::None);
    table.aligns.push(Align::None);
    for row in &mut table.rows {
        row.resize(columns + 1, String::new());
    }
    Some(render_table(&table))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cont(line: &str) -> Option<Enter> {
        enter(line, line.len())
    }

    #[test]
    fn a_bullet_continues() {
        assert_eq!(cont("- 項目"), Some(Enter::Continue("\n- ".to_owned())));
        assert_eq!(cont("  * 項目"), Some(Enter::Continue("\n  * ".to_owned())));
        assert_eq!(cont("+ a"), Some(Enter::Continue("\n+ ".to_owned())));
    }

    #[test]
    fn a_number_counts_up() {
        assert_eq!(cont("1. 項目"), Some(Enter::Continue("\n2. ".to_owned())));
        assert_eq!(cont("9) 項目"), Some(Enter::Continue("\n10) ".to_owned())));
    }

    /// **チェックは外して続ける。** 次の項目はまだ済んでいない
    #[test]
    fn a_task_continues_unchecked() {
        assert_eq!(
            cont("- [x] 済んだ"),
            Some(Enter::Continue("\n- [ ] ".to_owned()))
        );
        assert_eq!(
            cont("- [ ] まだ"),
            Some(Enter::Continue("\n- [ ] ".to_owned()))
        );
    }

    #[test]
    fn a_quote_continues_with_its_depth() {
        assert_eq!(cont("> 引用"), Some(Enter::Continue("\n> ".to_owned())));
        assert_eq!(cont("> > 二段"), Some(Enter::Continue("\n> > ".to_owned())));
        assert_eq!(
            cont("> - 引用の中のリスト"),
            Some(Enter::Continue("\n> - ".to_owned()))
        );
    }

    /// **中身の無い項目で押したら抜ける。**
    #[test]
    fn an_empty_item_ends_the_list() {
        assert_eq!(
            cont("- "),
            Some(Enter::Exit {
                with: String::new()
            })
        );
        assert_eq!(
            cont("  1. "),
            Some(Enter::Exit {
                with: "  ".to_owned()
            })
        );
        assert_eq!(
            cont("- [ ] "),
            Some(Enter::Exit {
                with: String::new()
            })
        );
        assert_eq!(
            cont("> "),
            Some(Enter::Exit {
                with: String::new()
            })
        );
        assert_eq!(
            cont("> > "),
            Some(Enter::Exit {
                with: ">".to_owned()
            })
        );
        assert_eq!(
            cont("> - "),
            Some(Enter::Exit {
                with: ">".to_owned()
            })
        );
    }

    #[test]
    fn plain_lines_are_left_alone() {
        for line in [
            "ふつうの文",
            "# 見出し",
            "---",
            "* * *",
            "-あ",
            "1.5 倍",
            "",
            "-",
            "1.",
        ] {
            assert_eq!(cont(line), None, "{line:?}");
        }
    }

    /// 記号の途中で押したら、ふつうの改行にする（記号を割らない）。
    #[test]
    fn pressing_inside_the_marker_is_a_plain_newline() {
        assert_eq!(enter("- 項目", 1), None);
        assert_eq!(enter("10. 項目", 2), None);
    }

    /// 項目の途中で押したら、後ろは次の項目へ送られる（記号を足すだけ）。
    #[test]
    fn pressing_in_the_middle_splits_the_item() {
        assert_eq!(
            enter("- 前後", "- 前".len()),
            Some(Enter::Continue("\n- ".to_owned()))
        );
    }

    #[test]
    fn line_comments_toggle_with_the_language_token() {
        let rust = comment_syntax("rust").expect("知っている");
        let added = toggle_line_comment("    let a = 1;\n    let b = 2;", rust);
        assert_eq!(added, "    // let a = 1;\n    // let b = 2;");
        assert_eq!(
            toggle_line_comment(&added, rust),
            "    let a = 1;\n    let b = 2;"
        );
    }

    /// **1 行でも違えば付ける**（VS Code と同じ）。浅い字下げに揃える
    #[test]
    fn mixed_lines_get_commented_at_the_shallowest_indent() {
        let python = comment_syntax("py").expect("知っている");
        assert_eq!(
            toggle_line_comment("# done\n  x = 1", python),
            "# # done\n#   x = 1"
        );
    }

    #[test]
    fn blank_lines_are_ignored_by_line_comments() {
        let rust = comment_syntax("rust").expect("知っている");
        assert_eq!(toggle_line_comment("a\n\nb", rust), "// a\n\n// b");
        assert_eq!(toggle_line_comment("// a\n\n// b", rust), "a\n\nb");
    }

    /// 本文の「行コメント」は行ごとに `<!-- -->` で包む。
    #[test]
    fn markdown_line_comments_wrap_each_line() {
        let added = toggle_line_comment("一行目\n二行目", MARKDOWN_COMMENT);
        assert_eq!(added, "<!-- 一行目 -->\n<!-- 二行目 -->");
        assert_eq!(
            toggle_line_comment(&added, MARKDOWN_COMMENT),
            "一行目\n二行目"
        );
    }

    #[test]
    fn block_comments_toggle() {
        let added = toggle_block_comment("一行目\n二行目\n", MARKDOWN_COMMENT);
        assert_eq!(added, "<!--\n一行目\n二行目\n-->\n");
        assert_eq!(
            toggle_block_comment(&added, MARKDOWN_COMMENT),
            "一行目\n二行目\n"
        );
        let c = comment_syntax("c").expect("知っている");
        assert_eq!(toggle_block_comment("x = 1;", c), "/* x = 1; */");
        assert_eq!(toggle_block_comment("/* x = 1; */", c), "x = 1;");
    }

    /// ブロックの書き方が無い言語では行コメントで代える。
    #[test]
    fn block_comments_fall_back_to_line_comments() {
        let python = comment_syntax("python").expect("知っている");
        assert_eq!(toggle_block_comment("x = 1", python), "# x = 1");
    }

    #[test]
    fn unknown_languages_have_no_syntax() {
        assert!(comment_syntax("なぞ").is_none());
        assert!(comment_syntax("rust,ignore").is_some(), "付記は落とす");
    }

    #[test]
    fn markers_are_added_and_removed() {
        assert_eq!(toggle_marker("前", "語", "後", "**"), Wrap::Add);
        assert_eq!(
            toggle_marker("前**", "語", "**後", "**"),
            Wrap::RemoveOutside
        );
        assert_eq!(
            toggle_marker("前", "**語**", "後", "**"),
            Wrap::RemoveInside
        );
        assert_eq!(toggle_marker("`", "code", "`", "`"), Wrap::RemoveOutside);
    }

    /// **太字の中で斜体を押しても、太字を壊さない。**
    #[test]
    fn italic_does_not_eat_bold() {
        assert_eq!(toggle_marker("**", "語", "**", "*"), Wrap::Add);
        assert_eq!(toggle_marker("***", "語", "***", "*"), Wrap::RemoveOutside);
        assert_eq!(toggle_marker("*", "語", "*", "**"), Wrap::Add);
    }

    #[test]
    fn urls_are_recognised() {
        assert!(looks_like_url("https://example.com/a?b=c"));
        assert!(looks_like_url("mailto:someone@example.com"));
        assert!(!looks_like_url("example.com"));
        assert!(!looks_like_url("https://a b"));
        assert!(!looks_like_url("https://"));
    }

    #[test]
    fn a_table_is_aligned() {
        let lines = ["|名前|値|", "|-|--:|", "|あ|1|", "|長い名前|12345|"];
        let formatted = format_table(&lines).expect("表");
        assert_eq!(formatted[0], "| 名前     |    値 |");
        assert_eq!(formatted[3], "| 長い名前 | 12345 |");
        // **縦が揃う**: どの行も同じ表示幅
        let widths: Vec<usize> = formatted.iter().map(|l| display_width(l)).collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{formatted:#?}");
        assert!(
            formatted[1].contains("--:"),
            "右寄せを保つ: {}",
            formatted[1]
        );
    }

    #[test]
    fn pipes_in_code_and_escapes_do_not_split_cells() {
        assert_eq!(split_row("| `a|b` | c\\|d |"), ["`a|b`", "c\\|d"]);
    }

    #[test]
    fn not_a_table_is_rejected() {
        assert!(format_table(&["| a |", "ただの行"]).is_none());
        assert!(format_table(&["| a |"]).is_none());
    }

    #[test]
    fn rows_and_columns_can_be_added() {
        let lines = ["| a | b |", "| --- | --- |", "| 1 | 2 |"];
        let rows = table_add_row(&lines).expect("表");
        assert_eq!(rows.len(), 4);
        let columns = table_add_column(&lines).expect("表");
        assert_eq!(split_row(&columns[0]).len(), 3);
        assert_eq!(split_row(&columns[2]).len(), 3);
    }

    /// 足りない枠は空で埋める（列の数を揃える）。
    #[test]
    fn short_rows_are_padded() {
        let formatted = format_table(&["| a | b |", "|---|---|", "| 1 |"]).expect("表");
        assert_eq!(split_row(&formatted[2]).len(), 2);
    }
}
