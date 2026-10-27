//! コードブロックの着色（設計メモ DEC-211）。
//!
//! **syntect から色そのものを取り出さない。** §16.10 のとおり、
//! 色は「役割」で持ち、実際の色は描画時にテーマで解決する。
//! ここではスコープ（`keyword.control.rust` など）を役割へ写すところまでを行う。
//!
//! これにより、テーマを同梱せずに済み（実行ファイルが小さく、起動も速い）、
//! PDF・HTML 出力でも同じ分類を使える。
//!
//! **着色は 1 ブロック 3〜7ms かかる**（§7.5 の実測）。
//! 1 フレームの予算 16.6ms に対して重いので、**レイアウトキャッシュ（§3.8）の
//! 内側で呼ぶ**。ブロックの `revision` が変わらない限り再着色は起きない。

use std::ops::Range;
use std::sync::OnceLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

/// 字句の役割。描画層がテーマで色に変える。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenRole {
    #[default]
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    /// 型・クラス
    Type,
    Function,
    /// `true` / `null` / 定数
    Constant,
    Punctuation,
}

/// 行の中の一区間。範囲はその行の先頭からのバイト位置。
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub range: Range<usize>,
    pub role: TokenRole,
}

/// 構文定義。**起動時にまとめて読む。**
///
/// §7.5 の実測で 75 種の読み込みが 1.7ms であり、遅延読み込みは要らない。
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// 既定の構文に無い言語（PowerShell・TOML。v2.8.1 R-01。`super::extra_syntax`）。
/// **既定の構文とは別の集まりにする**（既定に足すと全部を組み直すことになり、最初の着色で待たせる）
fn extra_syntaxes() -> &'static SyntaxSet {
    static EXTRA: OnceLock<SyntaxSet> = OnceLock::new();
    EXTRA.get_or_init(|| {
        let mut builder = syntect::parsing::SyntaxSetBuilder::new();
        for syntax in super::extra_syntax::all() {
            builder.add(syntax);
        }
        builder.build()
    })
}

/// 言語の名前から、構文とそれが入っている集まりを引く
fn find_syntax(
    name: &str,
) -> Option<(
    &'static SyntaxSet,
    &'static syntect::parsing::SyntaxReference,
)> {
    let lookup = |set: &'static SyntaxSet| {
        set.find_syntax_by_token(name)
            .or_else(|| set.find_syntax_by_extension(name))
            .map(|syntax| (set, syntax))
    };
    lookup(syntaxes()).or_else(|| lookup(extra_syntaxes()))
}

/// スコープの照合に使う前置き。**1 度だけ作る。**
///
/// 毎回文字列を組み立てると、字句の数だけ確保が走る。
struct Prefixes {
    comment: Scope,
    string: Scope,
    numeric: Scope,
    constant: Scope,
    keyword: Scope,
    storage: Scope,
    function: Scope,
    support_function: Scope,
    entity_type: Scope,
    class: Scope,
    support_type: Scope,
    punctuation: Scope,
}

fn prefixes() -> &'static Prefixes {
    static PREFIXES: OnceLock<Prefixes> = OnceLock::new();
    PREFIXES.get_or_init(|| {
        let scope = |name: &str| Scope::new(name).expect("スコープ名が正しい");
        Prefixes {
            comment: scope("comment"),
            string: scope("string"),
            numeric: scope("constant.numeric"),
            constant: scope("constant"),
            keyword: scope("keyword"),
            storage: scope("storage"),
            function: scope("entity.name.function"),
            support_function: scope("support.function"),
            entity_type: scope("entity.name.type"),
            class: scope("entity.name.class"),
            support_type: scope("support.type"),
            punctuation: scope("punctuation"),
        }
    })
}

/// いま積まれているスコープから役割を決める。
///
/// **コメントと文字列は「入れ物」なので、内側の種別より優先する。**
/// `// note` の `//` は `punctuation.definition.comment` であり、内側から見ると
/// 約物に当たってしまう。コメント 1 行はひとまとまりに見えるべきである。
///
/// それ以外は内側（具体的なほう）から見て、最初に当たったものを採る。
fn role_of(stack: &ScopeStack) -> TokenRole {
    let p = prefixes();
    let scopes = stack.as_slice();

    for scope in scopes {
        if p.comment.is_prefix_of(*scope) {
            return TokenRole::Comment;
        }
        if p.string.is_prefix_of(*scope) {
            return TokenRole::Str;
        }
    }

    for scope in scopes.iter().rev() {
        let scope = *scope;
        // **数値は定数より先に見る。** constant.numeric は constant にも当たる
        if p.numeric.is_prefix_of(scope) {
            return TokenRole::Number;
        }
        if p.function.is_prefix_of(scope) || p.support_function.is_prefix_of(scope) {
            return TokenRole::Function;
        }
        if p.entity_type.is_prefix_of(scope)
            || p.class.is_prefix_of(scope)
            || p.support_type.is_prefix_of(scope)
        {
            return TokenRole::Type;
        }
        if p.keyword.is_prefix_of(scope) || p.storage.is_prefix_of(scope) {
            return TokenRole::Keyword;
        }
        if p.constant.is_prefix_of(scope) {
            return TokenRole::Constant;
        }
        if p.punctuation.is_prefix_of(scope) {
            return TokenRole::Punctuation;
        }
    }
    TokenRole::Plain
}

/// フェンスの言語名を、syntect が知っている名前へ読み替える。
///
/// **syntect の既定構文は名前が 1 つしか無い。** `sh` や `py` のような
/// 文書でよく使う書き方は、そのままでは当たらない。
fn canonical(name: &str) -> &str {
    match name {
        "sh" | "shell" | "zsh" | "console" | "shell-session" => "bash",
        "py" | "python3" => "python",
        "rs" => "rust",
        "yml" => "yaml",
        "jsonc" | "json5" => "json",
        "htm" => "html",
        "c++" | "cxx" | "cc" => "cpp",
        "golang" => "go",
        "md" => "markdown",
        "rb" => "ruby",
        // **TypeScript の定義は入っていない。** JavaScript で代用する。
        // 型注釈は素の字になるが、無着色よりは読みやすい（DD-OPEN-12）
        "ts" | "typescript" | "tsx" => "javascript",
        // PowerShell（v2.8.1 R-01）
        "ps1" | "ps" | "pwsh" | "psm1" | "psd1" => "powershell",
        other => other,
    }
}

/// 意図して着色しない言語。**「解決できなかった」と区別する。**
fn is_plain_language(name: &str) -> bool {
    matches!(name, "text" | "txt" | "plain" | "plaintext" | "none")
}

/// コードを行ごとに着色する。
///
/// `language` はフェンスの言語名（`rust` / `python` など）。
/// **知らない言語でも失敗しない。** 着色せずに素の 1 区間として返す。
pub fn highlight(source: &str, language: Option<&str>) -> Vec<Vec<Token>> {
    let found = language
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|name| !name.is_empty() && !is_plain_language(name))
        .and_then(|name| find_syntax(canonical(&name)));

    let Some((set, syntax)) = found else {
        return plain(source);
    };

    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut lines = Vec::new();

    for line in source.split_inclusive('\n') {
        // **改行を含めて渡す。** load_defaults_newlines の定義は行末の改行を前提にする
        let Ok(ops) = state.parse_line(line, set) else {
            // 途中で壊れたら、そこから先は着色しない。**落とさない**
            lines.push(vec![whole(line)]);
            continue;
        };

        // **改行は区間に含めない。** コメントや文字列のスコープは改行まで伸びるため、
        // 切り詰めないと行からはみ出した範囲を返してしまう（実際に踏んだ）
        let end = line.trim_end_matches(['\n', '\r']).len();

        let mut tokens: Vec<Token> = Vec::new();
        let mut cursor = 0usize;
        for (index, op) in ops {
            let index = index.min(end);
            if index > cursor {
                push(&mut tokens, cursor..index, role_of(&stack));
                cursor = index;
            }
            if stack.apply(&op).is_err() {
                break;
            }
        }
        if cursor < end {
            push(&mut tokens, cursor..end, role_of(&stack));
        }
        lines.push(tokens);
    }
    // 足した構文は 1 行の中の決まりだけなので、行をまたぐ文字・注釈をここで補う（v2.8.1・v2.9.0 R-04）
    if let Some(rules) = super::extra_syntax::multiline_rules(&syntax.name) {
        span_lines(source, &mut lines, rules, |text| {
            let mut state = ParseState::new(syntax);
            let mut stack = ScopeStack::new();
            let Ok(ops) = state.parse_line(text, set) else {
                return vec![whole(text)];
            };
            let end = text.trim_end_matches(['\n', '\r']).len();
            let mut tokens = Vec::new();
            let mut cursor = 0usize;
            for (index, op) in ops {
                let index = index.min(end);
                if index > cursor {
                    push(&mut tokens, cursor..index, role_of(&stack));
                    cursor = index;
                }
                if stack.apply(&op).is_err() {
                    break;
                }
            }
            if cursor < end {
                push(&mut tokens, cursor..end, role_of(&stack));
            }
            tokens
        });
    }
    lines
}

/// 行をまたぐ文字・注釈（`"""` など）を、閉じるまで 1 つの役割で塗る（v2.9.0 R-04）。
/// `retokenize` は行の途中から色を付け直す（足した構文は文脈を持たないので、途中から始めても同じ答え）
fn span_lines(
    source: &str,
    lines: &mut [Vec<Token>],
    rules: &[super::extra_syntax::Multiline],
    retokenize: impl Fn(&str) -> Vec<Token>,
) {
    use super::extra_syntax::Multiline;
    // いま開いている決まり
    let mut open: Option<&Multiline> = None;
    for (line, tokens) in source.split_inclusive('\n').zip(lines.iter_mut()) {
        let end = line.trim_end_matches(['\n', '\r']).len();
        let text = &line[..end];
        // 開きを探し始める所（閉じた行では、閉じの印の後ろから）
        let mut scan_from = 0usize;
        if let Some(rule) = open {
            let close = if rule.close_at_start {
                let lead = text.len() - text.trim_start().len();
                text[lead..]
                    .starts_with(rule.close)
                    .then_some(lead + rule.close.len())
            } else {
                text.find(rule.close).map(|at| at + rule.close.len())
            };
            match close {
                None => {
                    *tokens = if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![Token {
                            range: 0..end,
                            role: rule.role,
                        }]
                    };
                    continue;
                }
                Some(stop) => {
                    let mut out = vec![Token {
                        range: 0..stop,
                        role: rule.role,
                    }];
                    for token in retokenize(&line[stop..]) {
                        push(
                            &mut out,
                            token.range.start + stop..token.range.end + stop,
                            token.role,
                        );
                    }
                    *tokens = out;
                    scan_from = stop;
                }
            }
        }
        // この行で開いたまま閉じない決まりがあるか
        open = rules.iter().find(|rule| {
            tokens
                .iter()
                .filter(|token| token.range.start >= scan_from)
                .any(|token| {
                    let part = &text[token.range.clone()];
                    if rule.close_at_start {
                        // here-string は行の終わりの印で開く
                        token.range.end == end && part.ends_with(rule.open)
                    } else {
                        part.starts_with(rule.open) && !part[rule.open.len()..].contains(rule.close)
                    }
                })
        });
    }
}

/// 設定「色を付ける拡張子」を読む（v2.8.0 R-01）。`rs, py, conf=sh` → `[(rs, None), (py, None), (conf, Some(sh))]`。
/// 拡張子は小文字にし、頭の `.` は外す
pub fn parse_extensions(setting: &str) -> Vec<(String, Option<String>)> {
    setting
        .split([',', ';'])
        .filter_map(|item| {
            let (extension, language) = match item.split_once('=') {
                Some((extension, language)) => (extension, Some(language.trim())),
                None => (item, None),
            };
            let extension = extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            (!extension.is_empty()).then(|| {
                (
                    extension,
                    language
                        .filter(|language| !language.is_empty())
                        .map(str::to_ascii_lowercase),
                )
            })
        })
        .collect()
}

/// Markdown 以外のファイルの言語（v2.8.0 R-01）。拡張子（無ければファイルの名前。`Makefile` など）から決め、
/// **着色できる言語のときだけ**返す。返す名前は [`highlight`] にそのまま渡せる。
///
/// `only` は設定「色を付ける拡張子」（[`parse_extensions`]）。**空でなければ、並べた拡張子だけ**に色を付ける。
/// `拡張子=言語` なら、その言語で色を付ける（知らない拡張子にも当てられる）
pub fn language_of_path(
    path: &std::path::Path,
    only: &[(String, Option<String>)],
) -> Option<String> {
    let set = syntaxes();
    let known = |name: &str| find_syntax(name).is_some();
    if !only.is_empty() {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let (_, language) = only.iter().find(|(listed, _)| *listed == extension)?;
        let name = canonical(language.as_deref().unwrap_or(&extension)).to_owned();
        return (known(&name) && !is_plain_language(&name)).then_some(name);
    }
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        let name = extension.to_ascii_lowercase();
        let name = canonical(&name).to_owned();
        let known = find_syntax(&name).is_some();
        return (known && !is_plain_language(&name)).then_some(name);
    }
    let name = path.file_name()?.to_str()?;
    set.find_syntax_by_extension(name).map(|_| name.to_owned())
}

/// 着色しない場合。行ごとに 1 区間だけ返す。
fn plain(source: &str) -> Vec<Vec<Token>> {
    source
        .split_inclusive('\n')
        .map(|line| vec![whole(line)])
        .collect()
}

fn whole(line: &str) -> Token {
    Token {
        range: 0..line.trim_end_matches(['\n', '\r']).len(),
        role: TokenRole::Plain,
    }
}

/// 同じ役割が続くならつなぐ。描画の回数を減らす。
fn push(tokens: &mut Vec<Token>, range: Range<usize>, role: TokenRole) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = tokens.last_mut() {
        if last.role == role && last.range.end == range.start {
            last.range.end = range.end;
            return;
        }
    }
    tokens.push(Token { range, role });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles_of(source: &str, language: &str) -> Vec<TokenRole> {
        highlight(source, Some(language))
            .into_iter()
            .flatten()
            .map(|token| token.role)
            .collect()
    }

    /// 足した PowerShell・TOML（v2.8.1 R-01）。1 行の中の決まりで色を付ける
    #[test]
    fn powershell_and_toml_are_colored() {
        let roles = |source: &str, language: &str, part: &str| -> Vec<TokenRole> {
            let lines: Vec<&str> = source.split_inclusive('\n').collect();
            let highlighted = highlight(source, Some(language));
            let mut out = Vec::new();
            for (line, tokens) in lines.iter().zip(&highlighted) {
                for token in tokens {
                    if line[token.range.clone()].contains(part) {
                        out.push(token.role);
                    }
                }
            }
            out
        };
        let ps = "# 注釈\nfunction Get-Thing([string]$Name) {\n  if ($Name -eq 'a') { Write-Output \"x $Name\" }\n  return $true\n}\n";
        assert!(roles(ps, "powershell", "注釈").contains(&TokenRole::Comment));
        assert!(
            roles(ps, "ps1", "function").contains(&TokenRole::Keyword),
            "ps1 でも引ける"
        );
        assert!(roles(ps, "pwsh", "Write-Output").contains(&TokenRole::Function));
        assert!(roles(ps, "powershell", "$Name").contains(&TokenRole::Type));
        assert!(roles(ps, "powershell", "-eq").contains(&TokenRole::Keyword));
        assert!(roles(ps, "powershell", "'a'").contains(&TokenRole::Str));
        assert!(roles(ps, "powershell", "$true").contains(&TokenRole::Constant));
        let toml = "# 注釈\n[package]\nname = \"mdview\"\nversion = '2.8.1'\nfast = true\nsize = 1_024\nwhen = 2026-10-26T10:00:00Z\n";
        assert!(roles(toml, "toml", "注釈").contains(&TokenRole::Comment));
        assert!(roles(toml, "toml", "[package]").contains(&TokenRole::Keyword));
        assert!(roles(toml, "toml", "name").contains(&TokenRole::Function));
        assert!(roles(toml, "toml", "\"mdview\"").contains(&TokenRole::Str));
        assert!(roles(toml, "toml", "true").contains(&TokenRole::Constant));
        assert!(roles(toml, "toml", "1_024").contains(&TokenRole::Number));
        assert!(roles(toml, "toml", "2026-10-26").contains(&TokenRole::Number));
        assert_covers(ps, Some("powershell"));
        assert_covers(toml, Some("toml"));
        assert_eq!(
            language_of_path(std::path::Path::new("a.psm1"), &[]).as_deref(),
            Some("powershell")
        );
        assert_eq!(
            language_of_path(std::path::Path::new("Cargo.toml"), &[]).as_deref(),
            Some("toml")
        );
    }

    /// 行をまたぐ文字・注釈は、閉じるまで同じ色（v2.9.0 R-04）
    #[test]
    fn multiline_strings_and_comments_span_lines() {
        let role_of_line = |source: &str, language: &str, number: usize| -> Vec<TokenRole> {
            highlight(source, Some(language))[number]
                .iter()
                .map(|token| token.role)
                .collect()
        };
        let toml = "a = \"\"\"\nname = 1\n\"\"\" # 閉じた\nb = 2\n";
        assert_eq!(
            role_of_line(toml, "toml", 1),
            [TokenRole::Str],
            "中の行は文字"
        );
        let closing = role_of_line(toml, "toml", 2);
        assert_eq!(closing.first(), Some(&TokenRole::Str));
        assert!(
            closing.contains(&TokenRole::Comment),
            "閉じたあとは色を付け直す"
        );
        assert!(
            role_of_line(toml, "toml", 3).contains(&TokenRole::Number),
            "閉じたあとの行はいつもどおり"
        );
        let ps = "<# 注釈の\n  続き if\n#>\nif ($a) { $s = @\"\n  中身 $x\n\"@ }\n";
        assert_eq!(role_of_line(ps, "powershell", 1), [TokenRole::Comment]);
        assert_eq!(role_of_line(ps, "powershell", 2), [TokenRole::Comment]);
        assert!(role_of_line(ps, "powershell", 3).contains(&TokenRole::Keyword));
        assert_eq!(
            role_of_line(ps, "powershell", 4),
            [TokenRole::Str],
            "here-string の中"
        );
        assert_eq!(
            role_of_line(ps, "powershell", 5).first(),
            Some(&TokenRole::Str)
        );
        assert_covers(toml, Some("toml"));
        assert_covers(ps, Some("powershell"));
    }

    /// 構文を足しても、読み込みは重くならない（最初の着色で待たせない）
    #[test]
    fn building_the_syntaxes_is_quick() {
        let started = std::time::Instant::now();
        let set = {
            let mut builder = syntect::parsing::SyntaxSetBuilder::new();
            for syntax in super::super::extra_syntax::all() {
                builder.add(syntax);
            }
            builder.build()
        };
        let took = started.elapsed();
        assert!(set.find_syntax_by_token("toml").is_some());
        eprintln!("構文の読み込み: {took:?}");
        assert!(took < std::time::Duration::from_millis(50), "{took:?}");
    }

    /// ファイルの言語は拡張子から決め、着色できないものは `None`（v2.8.0 R-01）
    #[test]
    fn languages_come_from_paths() {
        use std::path::Path;
        let of = |name: &str| language_of_path(Path::new(name), &[]);
        assert_eq!(of("src/main.rs").as_deref(), Some("rust"));
        assert_eq!(of("tool.PY").as_deref(), Some("python"));
        assert_eq!(of("a.json").as_deref(), Some("json"));
        assert_eq!(of("run.sh").as_deref(), Some("bash"));
        assert_eq!(of("notes.txt"), None, "テキストは着色しない");
        assert_eq!(of("data.unknownext"), None);
        assert!(of("Makefile").is_some(), "拡張子の無い名前");
        // 設定で並べたら、並べたものだけ。`拡張子=言語` で知らない拡張子にも当てる
        let only = parse_extensions(" .RS ; conf = sh,, py=");
        assert_eq!(
            only,
            [
                ("rs".to_owned(), None),
                ("conf".to_owned(), Some("sh".to_owned())),
                ("py".to_owned(), None)
            ]
        );
        let listed = |name: &str| language_of_path(Path::new(name), &only);
        assert_eq!(listed("a.rs").as_deref(), Some("rust"));
        assert_eq!(listed("a.json"), None, "並べていない");
        assert!(listed("app.conf").is_some(), "知らない拡張子に言語を当てる");
        assert_eq!(listed("Makefile"), None);
        let language = of("x.rs").unwrap();
        assert!(roles_of("fn main() {}\n", &language).contains(&TokenRole::Keyword));
    }

    /// 各行の区間が連続し、その行を覆う。**描画がこの対応に依存する。**
    fn assert_covers(source: &str, language: Option<&str>) {
        let lines: Vec<&str> = source.split_inclusive('\n').collect();
        let highlighted = highlight(source, language);
        assert_eq!(highlighted.len(), lines.len(), "行数が合わない");

        for (line, tokens) in lines.iter().zip(&highlighted) {
            let end = line.trim_end_matches(['\n', '\r']).len();
            let mut cursor = 0usize;
            for token in tokens {
                assert_eq!(token.range.start, cursor, "区間が連続していない: {line:?}");
                assert!(token.range.end <= end, "行からはみ出した: {line:?}");
                // 文字境界に乗っていること
                let _ = &line[token.range.clone()];
                cursor = token.range.end;
            }
            assert_eq!(cursor, end, "行末まで覆っていない: {line:?}");
        }
    }

    #[test]
    fn rust_keywords_and_strings() {
        let roles = roles_of("fn main() {\n    let s = \"hi\";\n}\n", "rust");
        assert!(roles.contains(&TokenRole::Keyword), "{roles:?}");
        assert!(roles.contains(&TokenRole::Str), "{roles:?}");
    }

    #[test]
    fn comments_are_marked() {
        let roles = roles_of("// これはコメント\nlet a = 1;\n", "rust");
        assert!(roles.contains(&TokenRole::Comment), "{roles:?}");
    }

    /// 数値は定数ではなく数値として扱う（照合の順序）。
    #[test]
    fn numbers_are_numbers_not_constants() {
        let roles = roles_of("let a = 42;\n", "rust");
        assert!(roles.contains(&TokenRole::Number), "{roles:?}");
    }

    #[test]
    fn python_is_supported() {
        let roles = roles_of("def f(x):\n    return \"a\"\n", "python");
        assert!(roles.contains(&TokenRole::Keyword), "{roles:?}");
        assert!(roles.contains(&TokenRole::Str), "{roles:?}");
    }

    /// **知らない言語でも落ちない。** 素のまま返す。
    #[test]
    fn unknown_language_falls_back_to_plain() {
        let lines = highlight("なにか\n", Some("存在しない言語"));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].role, TokenRole::Plain);
    }

    /// 文書でよく使う別名が当たる。
    #[test]
    fn common_aliases_resolve() {
        for name in ["sh", "shell", "py", "rs", "yml", "golang", "rb", "c++"] {
            let roles = roles_of(
                "x = 1
", name,
            );
            assert!(!roles.is_empty(), "{name} で何も返らない");
        }
        // 大文字でも当たる
        assert!(roles_of(
            "fn main() {}
",
            "Rust"
        )
        .contains(&TokenRole::Keyword));
    }

    /// `text` は**意図して**着色しない（解決できないのとは別）。
    #[test]
    fn plain_languages_are_not_highlighted() {
        for name in ["text", "txt", "plain", "none"] {
            let roles = roles_of(
                "fn main() {}
",
                name,
            );
            assert!(
                roles.iter().all(|r| *r == TokenRole::Plain),
                "{name} が着色された: {roles:?}"
            );
        }
    }

    #[test]
    fn no_language_is_plain() {
        let lines = highlight("abc\ndef\n", None);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().flatten().all(|t| t.role == TokenRole::Plain));
    }

    #[test]
    fn ranges_cover_every_line() {
        assert_covers("fn main() {\n    let s = \"hi\";\n}\n", Some("rust"));
        assert_covers("def f():\n    return 1\n", Some("python"));
        assert_covers("abc\ndef\n", None);
    }

    /// 日本語を含むコードでも文字境界を割らない。
    #[test]
    fn multibyte_source_is_safe() {
        assert_covers("// 日本語のコメント\nlet 名前 = \"値\";\n", Some("rust"));
    }

    #[test]
    fn empty_source_is_safe() {
        assert!(highlight("", Some("rust")).is_empty());
        assert!(highlight("", None).is_empty());
    }

    /// 改行で終わらない最終行も落とさない。
    #[test]
    fn last_line_without_newline() {
        let lines = highlight("let a = 1;", Some("rust"));
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].is_empty());
    }

    /// 同じ役割が続く区間はまとめる（描画回数を減らすため）。
    #[test]
    fn adjacent_tokens_are_merged() {
        let lines = highlight("// aaaa bbbb cccc\n", Some("rust"));
        assert_eq!(lines[0].len(), 1, "コメント 1 行が 1 区間になる");
    }
}
