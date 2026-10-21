//! 窓の題名とステータスバーの書式（v2.2.0）。
//!
//! **`{name}` のような差し込み口を、その場の値で置き換える。** 書式は設定画面で変えられ、
//! 既定の書式は v2.1 までの表示をそのまま再現する（[`DEFAULT_TITLE`]・[`DEFAULT_STATUS`]）。
//!
//! # 差し込み口
//!
//! |口|中身|
//! |---|---|
//! |`{name}`|ファイル名（無題なら「無題」）|
//! |`{path}`|フルパス（無題なら「無題」）|
//! |`{folder}`|置き場のフォルダ（無題なら空）|
//! |`{stem}`|拡張子を除いた名前|
//! |`{ext}`|拡張子（`md`。無ければ空）|
//! |`{type}`|ファイルの種別（`Markdown`・`テキスト`・`Rust` など）|
//! |`{dirty}`|未保存なら ` *`（前の空白も含む）|
//! |`{readonly}`|読み取り専用なら `［読み取り専用］ `（後ろの空白も含む）|
//! |`{state}`|保存済み・未保存・読み取り専用・未保存・読み取り専用|
//! |`{line}` `{col}` `{lines}`|カーソルの行・桁（1 始まり）・全体の行数|
//! |`{encoding}`|文字コード・BOM・改行|
//! |`{mode}`|表示モード（Edit / Preview / Split）|
//! |`{size}`|文書の大きさ|
//! |`{app}` `{version}`|`mdview`・版数|
//! |`{date}` `{date:書式}`|今日の日付。既定は `yyyy/mm/dd`|
//! |`{time}` `{time:書式}`|いまの時刻。既定は `hh:mm`|
//!
//! **`{date:…}` と `{time:…}` で `mm` の意味が違う**（日付では月、時刻では分）。
//! 1 つの書式で月と分を混ぜると読み違えるので、分けた。
//!
//! - 日付: `yyyy` `yy` `mm` `m` `dd` `d` `ddd`（曜日。`月`）
//! - 時刻: `hh` `h`（24 時間）`mm` `m` `ss` `s`
//!
//! それ以外の文字はそのまま出す。`{{` と `}}` は波括弧そのもの。
//! **知らない口は消さずに残す**（書き損じが画面で分かる）。

use std::path::Path;

use chrono::{Datelike, Local, Timelike};

/// v2.1 までの題名（`［読み取り専用］ a.md * — mdview`）。設定の既定と同じ
pub const DEFAULT_TITLE: &str = crate::io::settings::DEFAULT_TITLE_FORMAT;

/// v2.1 までのステータスバー（`a.md ・ 保存済み ・ Ln 1, Col 1 ・ UTF-8 / BOM なし / LF ・ Edit ・ 0 B`）。
pub const DEFAULT_STATUS: &str = crate::io::settings::DEFAULT_STATUS_FORMAT;

/// 設定画面に出す、使える口の一覧。
pub const HELP: &str =
    "使える口: {name} {path} {folder} {stem} {ext} {type} {dirty} {readonly} {state} \
{line} {col} {lines} {encoding} {mode} {size} {app} {version} {date} {date:yyyy-mm-dd ddd} \
{time} {time:hh:mm:ss}。{date} の mm は月、{time} の mm は分。";

/// 差し込む値の一式。
#[derive(Debug, Clone, Default)]
pub struct Fields<'a> {
    pub path: Option<&'a Path>,
    /// URL から開いたなら、その URL（v2.4.0 R-09）
    pub url: Option<&'a str>,
    pub dirty: bool,
    pub read_only: bool,
    pub line: usize,
    pub column: usize,
    pub lines: usize,
    pub encoding: String,
    pub mode: &'static str,
    pub size: String,
}

impl Fields<'_> {
    /// 保存状態の欄。**読み取り専用はここに出す**（いちばん目に入る場所）
    pub fn state(&self) -> &'static str {
        match (self.read_only, self.dirty) {
            (true, false) => "読み取り専用",
            (true, true) => "未保存・読み取り専用",
            (false, false) => "保存済み",
            (false, true) => "未保存",
        }
    }

    fn name(&self) -> String {
        self.path
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| self.url.map(crate::io::remote::file_name))
            .unwrap_or_else(|| "無題".to_owned())
    }

    /// 口の値。**知らない口は `None`**
    fn value(&self, key: &str) -> Option<String> {
        let path = self.path;
        Some(match key {
            "name" => self.name(),
            "path" => path
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "無題".to_owned()),
            "folder" => path
                .and_then(Path::parent)
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            "stem" => path
                .and_then(Path::file_stem)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "無題".to_owned()),
            "ext" => path
                .and_then(Path::extension)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "type" => file_kind(path).to_owned(),
            "dirty" => if self.dirty { " *" } else { "" }.to_owned(),
            "readonly" => if self.read_only {
                "［読み取り専用］ "
            } else {
                ""
            }
            .to_owned(),
            "state" => self.state().to_owned(),
            "line" => self.line.to_string(),
            "col" => self.column.to_string(),
            "lines" => self.lines.to_string(),
            "encoding" => self.encoding.clone(),
            "mode" => self.mode.to_owned(),
            "size" => self.size.clone(),
            "app" => super::APP_NAME.to_owned(),
            "version" => env!("CARGO_PKG_VERSION").to_owned(),
            _ => return None,
        })
    }
}

/// ファイルの種別（`{type}`）。**拡張子で決める**（中身は見ない）
pub fn file_kind(path: Option<&Path>) -> &'static str {
    let Some(path) = path else {
        return "Markdown";
    };
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "mdown" | "mkd" | "mkdn" => "Markdown",
        "rs" => "Rust",
        "py" | "pyw" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" | "jsx" => "React",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "C++",
        "cs" => "C#",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "sh" | "bash" | "zsh" => "シェル",
        "ps1" | "psm1" | "psd1" => "PowerShell",
        "bat" | "cmd" => "バッチ",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "json" | "jsonc" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" | "xaml" | "csproj" | "svg" => "XML",
        "ini" | "cfg" | "conf" => "設定",
        "csv" | "tsv" => "CSV",
        "log" => "ログ",
        _ => "テキスト",
    }
}

/// 書式を展開する。
pub fn expand(template: &str, fields: &Fields) -> String {
    expand_at(template, fields, Local::now().naive_local())
}

/// 書式を `{encoding}` の前・`{encoding}`・後ろの 3 つに分けて展開する。
///
/// **文字コードの欄だけを押せるようにする**（R-04）。`{encoding}` が無ければ真ん中は `None`
pub fn expand_parts(template: &str, fields: &Fields) -> (String, Option<String>, String) {
    match template.split_once("{encoding}") {
        Some((head, tail)) => (
            expand(head, fields),
            Some(fields.encoding.clone()),
            expand(tail, fields),
        ),
        None => (expand(template, fields), None, String::new()),
    }
}

fn expand_at(template: &str, fields: &Fields, now: chrono::NaiveDateTime) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(index) = rest.find(['{', '}']) {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if let Some(after) = tail.strip_prefix('}') {
            out.push('}');
            rest = after;
            continue;
        }
        // `{` … `}`。閉じていなければ、残りをそのまま出す
        let Some(close) = tail.find('}') else {
            out.push_str(tail);
            return out;
        };
        let inner = &tail[1..close];
        match placeholder(inner, fields, now) {
            Some(value) => out.push_str(&value),
            None => out.push_str(&tail[..=close]),
        }
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    out
}

fn placeholder(inner: &str, fields: &Fields, now: chrono::NaiveDateTime) -> Option<String> {
    let (key, format) = match inner.split_once(':') {
        Some((key, format)) => (key.trim(), Some(format)),
        None => (inner.trim(), None),
    };
    match key {
        "date" => Some(format_date(format.unwrap_or("yyyy/mm/dd"), now)),
        "time" => Some(format_time(format.unwrap_or("hh:mm"), now)),
        _ if format.is_some() => None,
        _ => fields.value(key),
    }
}

/// 書式の文字を長い順に当てる。
fn format_with(format: &str, tokens: &[(&str, &dyn Fn() -> String)]) -> String {
    let mut out = String::new();
    let mut rest = format;
    'outer: while let Some(c) = rest.chars().next() {
        for (token, value) in tokens {
            if rest.starts_with(token) {
                out.push_str(&value());
                rest = &rest[token.len()..];
                continue 'outer;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn format_date(format: &str, now: chrono::NaiveDateTime) -> String {
    const WEEKDAYS: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];
    let year = || now.year().to_string();
    let year2 = || format!("{:02}", now.year().rem_euclid(100));
    let month2 = || format!("{:02}", now.month());
    let month = || now.month().to_string();
    let weekday = || WEEKDAYS[now.weekday().num_days_from_monday() as usize].to_owned();
    let day2 = || format!("{:02}", now.day());
    let day = || now.day().to_string();
    format_with(
        format,
        &[
            ("yyyy", &year),
            ("yy", &year2),
            ("mm", &month2),
            ("m", &month),
            ("ddd", &weekday),
            ("dd", &day2),
            ("d", &day),
        ],
    )
}

fn format_time(format: &str, now: chrono::NaiveDateTime) -> String {
    let hour2 = || format!("{:02}", now.hour());
    let hour = || now.hour().to_string();
    let minute2 = || format!("{:02}", now.minute());
    let minute = || now.minute().to_string();
    let second2 = || format!("{:02}", now.second());
    let second = || now.second().to_string();
    format_with(
        format,
        &[
            ("hh", &hour2),
            ("h", &hour),
            ("mm", &minute2),
            ("m", &minute),
            ("ss", &second2),
            ("s", &second),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn at() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 3, 5)
            .unwrap()
            .and_hms_opt(9, 7, 4)
            .unwrap()
    }

    fn fields(path: &Path) -> Fields<'_> {
        Fields {
            url: None,
            path: Some(path),
            dirty: false,
            read_only: false,
            line: 12,
            column: 3,
            lines: 40,
            encoding: "UTF-8 / BOM なし / LF".to_owned(),
            mode: "Split",
            size: "2.0 KB".to_owned(),
        }
    }

    /// **既定の題名は v2.1 までと同じ**
    #[test]
    fn the_default_title_is_unchanged() {
        let path = PathBuf::from("C:/docs/a.md");
        let mut f = fields(&path);
        assert_eq!(expand_at(DEFAULT_TITLE, &f, at()), "a.md — mdview");
        f.dirty = true;
        assert_eq!(expand_at(DEFAULT_TITLE, &f, at()), "a.md * — mdview");
        f.read_only = true;
        assert_eq!(
            expand_at(DEFAULT_TITLE, &f, at()),
            "［読み取り専用］ a.md * — mdview"
        );
        let untitled = Fields {
            url: None,
            path: None,
            ..fields(&path)
        };
        assert_eq!(expand_at(DEFAULT_TITLE, &untitled, at()), "無題 — mdview");
    }

    /// **既定のステータスバーは v2.1 までと同じ**
    #[test]
    fn the_default_status_is_unchanged() {
        let path = PathBuf::from("C:/docs/報告書.md");
        assert_eq!(
            expand_at(DEFAULT_STATUS, &fields(&path), at()),
            "報告書.md ・ 保存済み ・ Ln 12, Col 3 ・ UTF-8 / BOM なし / LF ・ Split ・ 2.0 KB"
        );
    }

    #[test]
    fn file_placeholders() {
        let path = PathBuf::from("C:/docs/main.rs");
        let f = fields(&path);
        assert_eq!(
            expand_at("{stem}|{ext}|{type}|{lines}", &f, at()),
            "main|rs|Rust|40"
        );
        assert_eq!(expand_at("{path}", &f, at()), path.display().to_string());
        assert_eq!(file_kind(Some(Path::new("a.TXT"))), "テキスト");
        assert_eq!(file_kind(Some(Path::new("a.Markdown"))), "Markdown");
    }

    /// **日付の `mm` は月、時刻の `mm` は分**
    #[test]
    fn date_and_time() {
        let path = PathBuf::from("a.md");
        let f = fields(&path);
        assert_eq!(expand_at("{date}", &f, at()), "2026/03/05");
        assert_eq!(
            expand_at("{date:yyyy-mm-dd ddd}", &f, at()),
            "2026-03-05 木"
        );
        assert_eq!(expand_at("{date:yy年m月d日}", &f, at()), "26年3月5日");
        assert_eq!(expand_at("{time}", &f, at()), "09:07");
        assert_eq!(expand_at("{time:h時m分ss秒}", &f, at()), "9時7分04秒");
    }

    /// **知らない口は残す**。波括弧は 2 つ重ねると出る
    #[test]
    fn unknown_and_escapes() {
        let path = PathBuf::from("a.md");
        let f = fields(&path);
        assert_eq!(
            expand_at("{nope} {{name}} {name", &f, at()),
            "{nope} {name} {name"
        );
        assert_eq!(expand_at("}}x}", &f, at()), "}x}");
        assert_eq!(expand_at("{name:x}", &f, at()), "{name:x}");
    }

    /// 文字コードの欄で 3 つに分ける（押せる欄。R-04）
    #[test]
    fn parts_split_at_the_encoding() {
        let path = PathBuf::from("a.md");
        let f = fields(&path);
        let (head, middle, tail) = expand_parts(DEFAULT_STATUS, &f);
        assert_eq!(head, "a.md ・ 保存済み ・ Ln 12, Col 3 ・ ");
        assert_eq!(middle.as_deref(), Some("UTF-8 / BOM なし / LF"));
        assert_eq!(tail, " ・ Split ・ 2.0 KB");
        let (only, none, empty) = expand_parts("{name}", &f);
        assert_eq!((only.as_str(), none, empty.as_str()), ("a.md", None, ""));
    }
}
