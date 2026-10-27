//! 既定の構文に無い言語（v2.8.1 R-01。PowerShell・TOML）。
//!
//! **構文の定義ファイル（YAML）は持ち込まない。** syntect で YAML を読むには保守の止まった依存（yaml-rust）が要るので、
//! 定義をコードで組む。外から組めるのは 1 つの文脈の中の決まりだけ（文脈を積む `push` は syntect の外から作れない）なので、
//! **1 行の中の決まり**（注釈・文字・数・キーワード・変数など）に限る。複数行にまたがる文字・注釈は 1 行ずつ色を付ける。
//!
//! スコープの名前は `super::highlight::role_of` が役割に写す（色は持たない）。

use std::collections::HashMap;

use syntect::parsing::syntax_definition::{
    Context, MatchOperation, MatchPattern, Pattern, SyntaxDefinition,
};
use syntect::parsing::{Regex, Scope};

/// 決まり 1 つ（正規表現とスコープ）
fn rule(regex: &str, scope: &str) -> Pattern {
    Pattern::Match(MatchPattern {
        has_captures: false,
        regex: Regex::new(regex.to_owned()),
        scope: vec![Scope::new(scope).expect("スコープの名前")],
        captures: None,
        operation: MatchOperation::None,
        with_prototype: None,
    })
}

/// 決まりを並べた構文（入口の文脈 `__start` 1 つ。`main` も同じ中身にする）
fn syntax(name: &str, scope: &str, extensions: &[&str], rules: Vec<Pattern>) -> SyntaxDefinition {
    let mut context = Context::new(false);
    context.patterns = rules;
    let mut contexts = HashMap::new();
    contexts.insert("__start".to_owned(), context.clone());
    contexts.insert("__main".to_owned(), context.clone());
    contexts.insert("main".to_owned(), context);
    SyntaxDefinition {
        name: name.to_owned(),
        file_extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        scope: Scope::new(scope).expect("スコープの名前"),
        first_line_match: None,
        hidden: false,
        variables: HashMap::new(),
        contexts,
    }
}

/// PowerShell（`.ps1`・`.psm1`・`.psd1`）
fn powershell() -> SyntaxDefinition {
    syntax(
        "PowerShell",
        "source.powershell",
        &["ps1", "psm1", "psd1", "powershell", "pwsh"],
        vec![
            // 注釈（1 行の中で閉じる `<# #>` と、行の終わりまで）
            rule(r"<#.*?#>", "comment.block.powershell"),
            rule(r"<#.*", "comment.block.powershell"),
            rule(r"#.*", "comment.line.number-sign.powershell"),
            // 文字（`` ` `` で逃がす・`''` で逃がす・here-string の印）
            rule(r#"@"|"@"#, "string.quoted.double.heredoc.powershell"),
            rule(r"@'|'@", "string.quoted.single.heredoc.powershell"),
            rule(r#""(?:[^"`]|`.)*""#, "string.quoted.double.powershell"),
            rule(r"'(?:[^']|'')*'", "string.quoted.single.powershell"),
            // 定数の変数（`$true` など）は変数より先に
            rule(
                r"(?i)\$(?:true|false|null)\b",
                "constant.language.powershell",
            ),
            rule(
                r"\$(?:\{[^}]*\}|[A-Za-z_][\w:]*|[$?^_])",
                "support.type.variable.powershell",
            ),
            // 型（`[string]`・`[System.IO.File]`）
            rule(r"\[[A-Za-z_][\w.]*(?:\[\])?\]", "support.type.powershell"),
            // コマンドレット（`動詞-名詞`）
            rule(
                r"\b[A-Za-z]+-[A-Z][A-Za-z0-9]+\b",
                "support.function.powershell",
            ),
            // 比べる・つなぐ演算子
            rule(
                r"(?i)-(?:eq|ne|gt|ge|lt|le|like|notlike|match|notmatch|contains|notcontains|in|notin|replace|and|or|not|xor|is|isnot|as|f|split|join)\b",
                "keyword.operator.powershell",
            ),
            rule(
                r"(?i)\b(?:begin|break|catch|class|continue|data|default|do|dynamicparam|else|elseif|end|enum|exit|filter|finally|for|foreach|from|function|if|in|param|process|return|switch|throw|trap|try|until|using|var|while|workflow)\b",
                "keyword.control.powershell",
            ),
            rule(
                r"(?i)\b\d+(?:\.\d+)?(?:e[+-]?\d+)?(?:kb|mb|gb|tb|pb)?\b",
                "constant.numeric.powershell",
            ),
        ],
    )
}

/// TOML（`.toml`）
fn toml() -> SyntaxDefinition {
    syntax(
        "TOML",
        "source.toml",
        &["toml"],
        vec![
            rule(r"#.*", "comment.line.number-sign.toml"),
            // 表の見出し（`[名前]`・`[[名前]]`）
            rule(r"^\s*\[\[?[^\]\#]*\]\]?", "keyword.other.table.toml"),
            // 鍵（`名前 =` の名前）
            rule(
                r#"^\s*(?:"[^"]*"|'[^']*'|[A-Za-z0-9_.-]+)(?=\s*=)"#,
                "entity.name.function.key.toml",
            ),
            // 文字（複数行の `"""` `'''` は 1 行の中だけ）
            rule(r#"""".*?"""|""".*"#, "string.quoted.triple.toml"),
            rule(r"'''.*?'''|'''.*", "string.quoted.triple.literal.toml"),
            rule(r#""(?:[^"\\]|\\.)*""#, "string.quoted.double.toml"),
            rule(r"'[^']*'", "string.quoted.single.toml"),
            // 日付と時刻は数より先に
            rule(
                r"\b\d{4}-\d{2}-\d{2}(?:[Tt ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})?)?\b|\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b",
                "constant.numeric.date.toml",
            ),
            rule(r"\b(?:true|false)\b", "constant.language.toml"),
            rule(
                r"[+-]?\b(?:0x[0-9A-Fa-f_]+|0o[0-7_]+|0b[01_]+|\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?)\b|[+-]?\b(?:inf|nan)\b",
                "constant.numeric.toml",
            ),
        ],
    )
}

/// 足す構文の全部
pub fn all() -> Vec<SyntaxDefinition> {
    vec![powershell(), toml()]
}

/// 行をまたぐ文字・注釈（v2.9.0 R-04。開きと閉じの印・塗る役割）
pub struct Multiline {
    pub open: &'static str,
    pub close: &'static str,
    /// 閉じの印が行の頭にあるときだけ閉じる（PowerShell の here-string）
    pub close_at_start: bool,
    pub role: super::highlight::TokenRole,
}

/// 構文の名前ごとの、行をまたぐ決まり
pub fn multiline_rules(name: &str) -> Option<&'static [Multiline]> {
    use super::highlight::TokenRole::{Comment, Str};
    const POWERSHELL: [Multiline; 3] = [
        Multiline {
            open: "<#",
            close: "#>",
            close_at_start: false,
            role: Comment,
        },
        Multiline {
            open: "@\"",
            close: "\"@",
            close_at_start: true,
            role: Str,
        },
        Multiline {
            open: "@'",
            close: "'@",
            close_at_start: true,
            role: Str,
        },
    ];
    const TOML: [Multiline; 2] = [
        Multiline {
            open: "\"\"\"",
            close: "\"\"\"",
            close_at_start: false,
            role: Str,
        },
        Multiline {
            open: "'''",
            close: "'''",
            close_at_start: false,
            role: Str,
        },
    ];
    match name {
        "PowerShell" => Some(&POWERSHELL),
        "TOML" => Some(&TOML),
        _ => None,
    }
}
