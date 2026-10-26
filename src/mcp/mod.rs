//! MCP サーバー（v2.4.0 R-06。6a: stdio と、窓を出さない操作）。
//!
//! MCP クライアント（AI のアプリなど）が `mdview --mcp` を起こし、標準入出力で MCP（JSON-RPC）を話す。
//! **画面のボタンを押させるのではなく、文書の構造を取る・検査する・比べる、などの「意味のある操作」**を出す
//! （MarkUpsideDown の方針と同じ）。実装は公式の Rust SDK（`rmcp`）。
//!
//! 6a では**ファイルを書き換える操作は出さない**（本文を返すだけ）。保存・出力と「許可」「確認」「拒否」は 6c、
//! 開いている窓へつなぐのは 6b、手元の HTTP は 6d。
//!
//! 操作の中身は窓と同じ関数を使う（検査 `edit::lint`、目次と番号 `edit::toc`、パッチ `diff::patch`）。
//! 検査の規則のオン・オフと目次の深さは、利用者の設定（`settings.toml`）に従う。

use std::path::Path;

pub mod http;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::io::settings::Settings;

/// `mdview --mcp` の入口。**窓は出さない。** 終わったときの終了コードを返す
pub fn run() -> i32 {
    // **設定で切っていれば、つながずに理由を返して終わる**（6c）
    if !Settings::load().mcp_enabled {
        eprintln!(
            "mdview --mcp: 設定で MCP サーバーを切っています（mdview の設定 → MCP で入れられます）"
        );
        return 1;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mdview --mcp: 起動できません: {error}");
            return 1;
        }
    };
    let result = runtime.block_on(async {
        let service = MdviewServer::new()
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|error| error.to_string())?;
        service.waiting().await.map_err(|error| error.to_string())?;
        Ok::<(), String>(())
    });
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("mdview --mcp: {error}");
            1
        }
    }
}

/// 文書の渡し方。**ファイルのパスか本文のどちらか**（両方なら本文を使う）
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct Source {
    #[schemars(description = "Markdown ファイルのパス（text と、どちらか一方）")]
    pub path: Option<String>,
    #[schemars(description = "Markdown の本文（path と、どちらか一方。両方あれば text を使う）")]
    pub text: Option<String>,
    #[schemars(
        description = "true なら、開いている mdview の窓の本文を使う（pid の窓、無ければ select_window で選んだ窓、窓が 1 つならそれ）。本文を書き換える道具は、結果をその窓へ書き戻す（元に戻すで 1 回で戻る）"
    )]
    pub window: Option<bool>,
    #[schemars(description = "窓のプロセス番号（list_windows で分かる。window=true のとき）")]
    pub pid: Option<u32>,
}

impl Source {
    /// 本文と、読んだときの形（パスで渡したときだけ）
    fn read(&self) -> Result<(String, Option<crate::io::LoadedFile>), String> {
        if let Some(text) = &self.text {
            return Ok((text.replace("\r\n", "\n"), None));
        }
        let Some(path) = self.path.as_deref().filter(|p| !p.trim().is_empty()) else {
            return Err("path か text のどちらかを渡してください".to_owned());
        };
        let loaded = crate::io::load(Path::new(path))
            .map_err(|error| format!("読めません（{path}）: {error}"))?;
        Ok((loaded.text.clone(), Some(loaded)))
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ReadRequest {
    #[schemars(description = "読む Markdown（テキスト）ファイルのパス")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct LintRequest {
    #[serde(flatten)]
    pub source: Source,
    #[schemars(
        description = "true なら、直し方が 1 つに決まる指摘を直した本文も返す（ファイルは書き換えない）"
    )]
    pub fix: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CompareRequest {
    #[schemars(description = "元（左）のファイルのパス（left_text と、どちらか一方）")]
    pub left_path: Option<String>,
    #[schemars(description = "元（左）の本文")]
    pub left_text: Option<String>,
    #[schemars(description = "新（右）のファイルのパス（right_text と、どちらか一方）")]
    pub right_path: Option<String>,
    #[schemars(description = "新（右）の本文")]
    pub right_text: Option<String>,
    #[schemars(description = "パッチの形式: unified（既定）か context")]
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct PatchRequest {
    #[serde(flatten)]
    pub source: Source,
    #[schemars(description = "当てるパッチ（unified か context。形式は中身から見分ける）")]
    pub patch: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TocRequest {
    #[serde(flatten)]
    pub source: Source,
    #[schemars(description = "目次に入れる見出しの深さ（1〜6。省くと設定の値）")]
    pub depth: Option<u8>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct NumberRequest {
    #[serde(flatten)]
    pub source: Source,
    #[schemars(description = "true なら番号を外す（省くと振る）")]
    pub remove: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct WindowRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SelectRequest {
    #[schemars(description = "選ぶ窓のプロセス番号（list_windows で分かる）")]
    pub pid: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ReplaceRangeRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "置き換える範囲の始まりの行（1 から）")]
    pub start_line: usize,
    #[schemars(description = "始まりの桁（1 から。文字の数）")]
    pub start_column: usize,
    #[schemars(description = "終わりの行（1 から）")]
    pub end_line: usize,
    #[schemars(description = "終わりの桁（1 から。この桁の文字の手前まで）")]
    pub end_column: usize,
    #[schemars(description = "代わりに入れる文字")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct InsertRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "入れる行（1 から）")]
    pub line: usize,
    #[schemars(description = "入れる桁（1 から。この桁の文字の手前に入る）")]
    pub column: usize,
    #[schemars(description = "入れる文字")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ReplaceAllRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "新しい本文（全文）")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "探す文字（regex=true なら正規表現）")]
    pub query: String,
    #[schemars(description = "true なら query を正規表現として扱う")]
    pub regex: Option<bool>,
    #[schemars(description = "true なら大文字と小文字を区別する（省くと区別しない）")]
    pub case_sensitive: Option<bool>,
    #[schemars(
        description = "置き換える文字（replace_in_window のとき。正規表現なら $1 などが使える）"
    )]
    pub replacement: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SaveAsRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "保存先のパス")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ExportRequest {
    #[serde(flatten)]
    pub source: Source,
    #[schemars(
        description = "出力の形式: pdf・html・pptx（pptx は Marp の文書だけ。Marp の文書は pdf・html もスライドとして書き出す）"
    )]
    pub format: String,
    #[schemars(description = "書き出すファイルのパス")]
    pub output_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct PathRequest {
    #[schemars(description = "開くファイルのパス")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct OpenInWindowRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "その窓で開くファイルのパス（いまの文書の未保存の変更は失われる）")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct UrlRequest {
    #[schemars(description = "開く URL（http:// か https://）")]
    pub url: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct LinkRequest {
    #[schemars(description = "開く先（URL かファイルのパス。OS の既定のアプリで開く）")]
    pub target: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SettingRequest {
    #[schemars(
        description = "設定の鍵（settings.toml の鍵。例: theme・tab_width。mcp_ で始まるものは変えられない）"
    )]
    pub key: String,
    #[schemars(description = "値（真偽は true / false、数はそのまま、文字はそのまま）")]
    pub value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ReadOnlyRequest {
    #[schemars(description = "窓のプロセス番号（省くと、選んだ窓か、窓が 1 つならそれ）")]
    pub pid: Option<u32>,
    #[schemars(description = "true で読み取り専用にする、false で外す")]
    pub on: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ClipboardRequest {
    #[schemars(description = "クリップボードへ写す文字")]
    pub text: String,
}

/// 設定の値を設定ファイルの書き方にする（真偽と数はそのまま、文字は引用符で囲む）
fn toml_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value == "true" || value == "false" || value.parse::<f64>().is_ok() {
        return Some(value.to_owned());
    }
    // **`"` と改行は書けない**（読み戻せない）
    if value.contains('"') || value.contains('\n') || value.contains('\r') {
        return None;
    }
    Some(format!("\"{value}\""))
}

/// 道具の呼び出しを、確認のダイアログに出す言葉にする
fn describe(tool: &str, arguments: &rmcp::model::JsonObject) -> String {
    let label = crate::io::mcp_access::TOOLS
        .iter()
        .find(|(name, _, _)| *name == tool)
        .map_or(tool, |(_, label, _)| *label);
    let mut details = Vec::new();
    for key in [
        "pid",
        "path",
        "output_path",
        "target",
        "key",
        "value",
        "format",
        "query",
        "replacement",
    ] {
        if let Some(value) = arguments.get(key) {
            let shown = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            let shown: String = shown.chars().take(80).collect();
            details.push(format!("{key}: {shown}"));
        }
    }
    if let Some(Value::String(text)) = arguments.get("text") {
        details.push(format!("text: {} 文字", text.chars().count()));
    }
    if details.is_empty() {
        label.to_owned()
    } else {
        format!("{label}（{}）", details.join("、"))
    }
}

/// 探す文字を正規表現にする
fn search_regex(query: &str, regex: bool, case_sensitive: bool) -> Result<regex::Regex, String> {
    let pattern = if regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|error| format!("探す文字を読めません: {error}"))
}

/// 一致した所（行・桁は 1 から。多すぎる文書では先頭の 200 件）
fn matches_of(text: &str, regex: &regex::Regex) -> Vec<Value> {
    regex
        .find_iter(text)
        .take(200)
        .map(|found| {
            let (line, column) = crate::app::position_of(text, found.start());
            json!({ "line": line, "column": column, "text": found.as_str() })
        })
        .collect()
}

fn ok_json(value: Value) -> Result<CallToolResult, ErrorData> {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn ok_text(text: String) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// 操作としての失敗（**MCP の誤りではない**。理由を AI に返す）
fn failed(reason: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![ContentBlock::text(
        reason.into(),
    )]))
}

/// 文書の構造（見出し・リンク・ブロック・frontmatter・統計）
pub fn structure(text: &str) -> Value {
    use crate::edit::navigate::{links_in, LinkKind};
    use crate::parse::BlockKind;

    let headings: Vec<Value> = crate::edit::toc::headings(text)
        .into_iter()
        .map(|h| json!({ "line": h.line + 1, "level": h.level, "title": h.title }))
        .collect();
    let mut links = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        for link in links_in(line) {
            let (kind, target) = match link.kind {
                LinkKind::Inline { target } => ("inline", target),
                LinkKind::Reference { label } => ("reference", label),
                LinkKind::Footnote { label } => ("footnote", label),
            };
            links.push(json!({
                "line": index + 1,
                "kind": kind,
                "target": target,
                "text": &line[link.range.clone()],
            }));
        }
    }
    let document = crate::document::Document::from_text(text.to_owned());
    let mut code_blocks = Vec::new();
    let (mut tables, mut lists, mut quotes, mut paragraphs) = (0, 0, 0, 0);
    for block in document.blocks() {
        match &block.kind {
            BlockKind::Code { language } => code_blocks.push(json!({
                "line": block.start_line + 1,
                "lines": block.line_count,
                "language": language,
            })),
            BlockKind::Table => tables += 1,
            BlockKind::List => lists += 1,
            BlockKind::Quote => quotes += 1,
            BlockKind::Paragraph => paragraphs += 1,
            _ => {}
        }
    }
    json!({
        "headings": headings,
        "links": links,
        "code_blocks": code_blocks,
        "frontmatter": frontmatter(text),
        "stats": {
            "lines": text.split('\n').count(),
            "characters": text.chars().count(),
            "bytes": text.len(),
            "paragraphs": paragraphs,
            "tables": tables,
            "lists": lists,
            "quotes": quotes,
        },
    })
}

/// 先頭の `---`（TOML なら `+++`。v2.7.0）で囲んだ frontmatter（無ければ `null`）。**中身は文字のまま返す**（YAML・TOML は読まない）
fn frontmatter(text: &str) -> Value {
    let mut lines = text.split('\n');
    let fence = match lines.next().map(str::trim_end) {
        Some(fence @ ("---" | "+++")) => fence,
        _ => return Value::Null,
    };
    let mut body = Vec::new();
    for line in lines {
        if line.trim_end() == fence {
            return json!(body.join("\n"));
        }
        body.push(line);
    }
    Value::Null
}

/// mdview の MCP サーバー
#[derive(Clone)]
pub struct MdviewServer {
    tool_router: ToolRouter<Self>,
    /// select_window で選んだ窓（6b）
    selected: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
    /// クリップボード（**持ち続ける**。X11 では持ち主がいなくなると中身が消える）
    clipboard: std::sync::Arc<std::sync::Mutex<Option<arboard::Clipboard>>>,
}

impl std::fmt::Debug for MdviewServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdviewServer").finish_non_exhaustive()
    }
}

impl MdviewServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            selected: std::sync::Arc::default(),
            clipboard: std::sync::Arc::default(),
        }
    }

    /// 開いている窓（答えたもの）の様子。**つなげなかった居場所は消す**（落ちて残ったもの）
    async fn live_windows(&self) -> Vec<Value> {
        let mut windows = Vec::new();
        for pid in crate::ipc::registered() {
            match crate::ipc::request(pid, &json!({ "cmd": "info" })).await {
                Ok(info) => windows.push(info),
                Err(_) => crate::ipc::unregister(pid),
            }
        }
        windows
    }

    /// 相手の窓。**pid、選んだ窓、窓が 1 つならそれ**の順
    async fn target(&self, pid: Option<u32>) -> Result<u32, String> {
        if let Some(pid) = pid {
            return Ok(pid);
        }
        if let Some(pid) = self.selected.lock().ok().and_then(|selected| *selected) {
            return Ok(pid);
        }
        let windows = self.live_windows().await;
        match windows.as_slice() {
            [] => Err("開いている mdview の窓がありません".to_owned()),
            [only] => Ok(only["pid"].as_u64().unwrap_or(0) as u32),
            _ => Err(format!(
                "窓が {} つ開いています。list_windows で選び、select_window か pid で指定してください",
                windows.len()
            )),
        }
    }

    /// **呼ぶ前に、操作ごとの扱い（許可・確認・拒否）を見る**（6c。すべての道具がここを通る）
    async fn gate(&self, tool: &str, arguments: &rmcp::model::JsonObject) -> Result<(), String> {
        use crate::io::mcp_access::{log, permission_of, Permission};

        let settings = Settings::load();
        let detail = describe(tool, arguments);
        // **MCP 自身の扱いは AI から変えさせない**。断ると決まっているので、利用者には聞かない
        if tool == "set_setting"
            && arguments
                .get("key")
                .and_then(Value::as_str)
                .is_some_and(|key| key.trim().starts_with("mcp_"))
        {
            log(tool, Permission::Deny, "拒否（MCP の設定）", &detail);
            return Err(
                "MCP の設定（mcp_ で始まる鍵）は、mdview の設定画面で変えてください".to_owned(),
            );
        }
        let window_flag = arguments.get("window").and_then(Value::as_bool) == Some(true);
        let pid = arguments
            .get("pid")
            .and_then(Value::as_u64)
            .map(|pid| pid as u32);
        // **窓が無いときは受けない設定**
        if !settings.mcp_headless && self.live_windows().await.is_empty() {
            log(tool, Permission::Deny, "窓が無いので断った", &detail);
            return Err(
                "mdview の設定で、窓が開いていないときは受けないようにしています".to_owned(),
            );
        }
        let mut permission = permission_of(&settings.mcp_permissions, tool);
        // 窓の本文を書き換える呼び出しか
        let writes_window = matches!(
            tool,
            "replace_range" | "insert_text" | "replace_window_text" | "replace_in_window"
        ) || (window_flag
            && matches!(tool, "update_toc" | "number_headings" | "apply_patch"))
            || (window_flag
                && tool == "lint_markdown"
                && arguments.get("fix").and_then(Value::as_bool) == Some(true));
        let window = if writes_window || window_flag || pid.is_some() {
            self.target(pid).await.ok()
        } else {
            None
        };
        if permission == Permission::Allow && writes_window {
            // **パッチを窓の本文へ当てるのは「確認」**（多くの行が一度に変わる。要件定義書 R-06）
            if tool == "apply_patch" {
                permission = Permission::Confirm;
            }
            // **自動保存を入れている窓を書き換えるなら「確認」**（N 秒でファイルに書かれる）
            if let Some(pid) = window {
                if let Ok(info) = crate::ipc::request(pid, &json!({ "cmd": "info" })).await {
                    if info["autosave"] == Value::Bool(true) {
                        permission = Permission::Confirm;
                    }
                }
            }
        }
        match permission {
            Permission::Allow => {
                log(tool, permission, "許可", &detail);
                Ok(())
            }
            Permission::Deny => {
                log(tool, permission, "拒否（設定）", &detail);
                Err(format!("mdview の設定で「{tool}」を拒否しています"))
            }
            Permission::Confirm => {
                if self.ask(window, tool, &detail).await {
                    log(tool, permission, "確認で許可", &detail);
                    Ok(())
                } else {
                    log(tool, permission, "確認で拒否", &detail);
                    Err(format!("利用者が「{tool}」を拒否しました"))
                }
            }
        }
    }

    /// 利用者に確かめる。**窓があればその窓に、無ければ OS のダイアログで**
    async fn ask(&self, window: Option<u32>, tool: &str, detail: &str) -> bool {
        let pid = match window {
            Some(pid) => Some(pid),
            None => self.target(None).await.ok(),
        };
        if let Some(pid) = pid {
            let message = json!({ "cmd": "confirm", "tool": tool, "message": detail });
            if let Ok(answer) = crate::ipc::request(pid, &message).await {
                return answer["allowed"] == Value::Bool(true);
            }
        }
        // **窓が無いときだけ、そのときだけの確認の窓を出す**（OS のダイアログ）
        let description = format!(
            "{detail}\n\n（MCP の操作: {tool}。扱いは mdview の設定の「MCP」で変えられます）"
        );
        tokio::task::spawn_blocking(move || {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("mdview: AI が操作しようとしています")
                .set_description(description)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
        })
        .await
        .unwrap_or(false)
    }

    /// 窓に頼む（答えの `ok` は除く）
    async fn ask_window(
        &self,
        pid: Option<u32>,
        message: Value,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        match crate::ipc::request(pid, &message).await {
            Ok(mut answer) => {
                if let Some(map) = answer.as_object_mut() {
                    map.remove("ok");
                    map.insert("window".to_owned(), json!(pid));
                }
                ok_json(answer)
            }
            Err(reason) => failed(reason),
        }
    }

    /// 本文と、窓から読んだならその窓（**書き戻す先**）
    async fn read_source(&self, source: &Source) -> Result<(String, Option<u32>), String> {
        if source.window == Some(true)
            || (source.pid.is_some() && source.text.is_none() && source.path.is_none())
        {
            let pid = self.target(source.pid).await?;
            let answer = crate::ipc::request(pid, &json!({ "cmd": "get_text" })).await?;
            return Ok((answer["text"].as_str().unwrap_or("").to_owned(), Some(pid)));
        }
        source.read().map(|(text, _)| (text, None))
    }

    /// 窓の本文を置き換える（変わった範囲だけを 1 つの編集にする）
    async fn write_window(&self, pid: u32, text: &str) -> Result<Value, String> {
        crate::ipc::request(pid, &json!({ "cmd": "replace_all", "text": text })).await
    }

    /// 本文を返す道具の答え。**窓から読んだなら書き戻して、その旨を返す**
    async fn transformed(
        &self,
        text: String,
        window: Option<u32>,
    ) -> Result<CallToolResult, ErrorData> {
        match window {
            Some(pid) => match self.write_window(pid, &text).await {
                Ok(answer) => ok_json(json!({
                    "written_to_window": pid,
                    "changed": answer["changed"],
                    "text": text,
                })),
                Err(reason) => failed(reason),
            },
            None => ok_text(text),
        }
    }
}

impl Default for MdviewServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl MdviewServer {
    #[tool(
        description = "Markdown（テキスト）ファイルを読む。文字コードを判定して開き、本文と、文字コード・BOM・改行コード・行数を返す。ファイルは書き換えない"
    )]
    fn read_document(
        &self,
        Parameters(ReadRequest { path }): Parameters<ReadRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        match crate::io::load(Path::new(&path)) {
            Ok(loaded) => ok_json(json!({
                "path": path,
                "encoding": loaded.format.encoding.label(),
                "bom": loaded.format.has_bom,
                "line_ending": loaded.format.line_ending.label(),
                "lines": loaded.text.split('\n').count(),
                "text": loaded.text,
            })),
            Err(error) => failed(format!("読めません（{path}）: {error}")),
        }
    }

    #[tool(
        description = "Markdown の構造を JSON で返す: 見出し（行・段・題）、リンク（行・種類・行き先）、コードブロック（行・言語）、frontmatter、統計（行数・文字数・段落・表・リスト・引用の数）"
    )]
    async fn document_structure(
        &self,
        Parameters(source): Parameters<Source>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.read_source(&source).await {
            Ok((text, _)) => ok_json(structure(&text)),
            Err(reason) => failed(reason),
        }
    }

    #[tool(
        description = "Markdown を検査する（markdownlint の規則のうち 17 個。規則のオン・オフは mdview の設定に従う）。指摘（規則・行・説明・直せるか）を返す。fix=true なら直した本文も返す（ファイルは書き換えない）"
    )]
    async fn lint_markdown(
        &self,
        Parameters(LintRequest { source, fix }): Parameters<LintRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (text, window) = match self.read_source(&source).await {
            Ok(read) => read,
            Err(reason) => return failed(reason),
        };
        let settings = Settings::load();
        let disabled = if settings.lint_enabled {
            settings.lint_disabled.clone()
        } else {
            Vec::new()
        };
        let diagnostics = crate::edit::lint::lint(&text, &disabled);
        let found: Vec<Value> = diagnostics
            .iter()
            .map(|d| {
                json!({
                    "rule": d.rule,
                    "line": d.line + 1,
                    "description": crate::edit::lint::description(d.rule),
                    "fixable": d.fix.is_some(),
                })
            })
            .collect();
        let mut answer = json!({ "count": found.len(), "diagnostics": found });
        if fix == Some(true) {
            let fixed = crate::edit::lint::apply_fixes(&text, &diagnostics);
            // **窓から読んだなら、直した本文を窓へ書き戻す**
            if let Some(pid) = window {
                if let Err(reason) = self.write_window(pid, &fixed).await {
                    return failed(reason);
                }
                answer["written_to_window"] = json!(pid);
            }
            answer["fixed_text"] = json!(fixed);
        }
        ok_json(answer)
    }

    #[tool(description = "2 つの文書を比べ、パッチ（unified か context）を返す。違いが無ければ空")]
    fn compare_documents(
        &self,
        Parameters(request): Parameters<CompareRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let left = Source {
            path: request.left_path.clone(),
            text: request.left_text.clone(),
            window: None,
            pid: None,
        };
        let right = Source {
            path: request.right_path.clone(),
            text: request.right_text.clone(),
            window: None,
            pid: None,
        };
        let (old, new) = match (left.read(), right.read()) {
            (Ok((old, _)), Ok((new, _))) => (old, new),
            (Err(reason), _) | (_, Err(reason)) => return failed(reason),
        };
        let format = match request.format.as_deref().map(str::trim) {
            Some("context") => crate::diff::patch::Format::Context,
            None | Some("") | Some("unified") => crate::diff::patch::Format::Unified,
            Some(other) => return failed(format!("format は unified か context です: {other}")),
        };
        let name = |path: &Option<String>, fallback: &str| {
            path.clone().unwrap_or_else(|| fallback.to_owned())
        };
        ok_text(crate::diff::patch::write(
            &old,
            &new,
            &name(&request.left_path, "a"),
            &name(&request.right_path, "b"),
            format,
        ))
    }

    #[tool(
        description = "パッチを文書に当てた結果の本文を返す（ファイルは書き換えない）。当てられた部分と当てられなかった部分の数も返す"
    )]
    async fn apply_patch(
        &self,
        Parameters(PatchRequest { source, patch }): Parameters<PatchRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (text, window) = match self.read_source(&source).await {
            Ok(read) => read,
            Err(reason) => return failed(reason),
        };
        let patches = match crate::diff::patch::parse(&patch) {
            Ok(patches) => patches,
            Err(error) => return failed(format!("パッチを読めません: {error}")),
        };
        // **ファイルが 1 つならそれ、複数ならパスの名前が合うもの**
        let chosen = match patches.len() {
            0 => return failed("パッチに当てる部分がありません"),
            1 => &patches[0],
            _ => {
                let wanted = source
                    .path
                    .as_deref()
                    .and_then(|p| Path::new(p).file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                match patches.iter().find(|p| {
                    p.names()
                        .iter()
                        .any(|n| !wanted.is_empty() && n.ends_with(wanted))
                }) {
                    Some(found) => found,
                    None => return failed(
                        "パッチに複数のファイルがあります。当てるファイルを path で渡してください",
                    ),
                }
            }
        };
        let applied = crate::diff::patch::apply(&text, chosen);
        let mut answer = json!({
            "applied": applied.applied,
            "failed": applied.failed.len(),
            "text": applied.text,
        });
        if let Some(pid) = window {
            if let Err(reason) = self.write_window(pid, &applied.text).await {
                return failed(reason);
            }
            answer["written_to_window"] = json!(pid);
        }
        ok_json(answer)
    }

    #[tool(
        description = "目次を入れる・作り直す（<!-- TOC --> と <!-- /TOC --> の間だけ。無ければ先頭の見出しの後に入れる）。結果の本文を返す（ファイルは書き換えない）"
    )]
    async fn update_toc(
        &self,
        Parameters(TocRequest { source, depth }): Parameters<TocRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (text, window) = match self.read_source(&source).await {
            Ok(read) => read,
            Err(reason) => return failed(reason),
        };
        let depth = depth
            .unwrap_or_else(|| Settings::load().toc_depth)
            .clamp(1, 6);
        let updated = crate::edit::toc::update(&text, depth)
            .unwrap_or_else(|| crate::edit::toc::block(&text, depth));
        self.transformed(updated, window).await
    }

    #[tool(
        description = "見出しに番号を振る（1. 1.1. の形）、または外す（remove=true）。結果の本文を返す（ファイルは書き換えない）"
    )]
    async fn number_headings(
        &self,
        Parameters(NumberRequest { source, remove }): Parameters<NumberRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.read_source(&source).await {
            Ok((text, window)) => {
                let updated = if remove == Some(true) {
                    crate::edit::toc::unnumber(&text)
                } else {
                    crate::edit::toc::number(&text)
                };
                self.transformed(updated, window).await
            }
            Err(reason) => failed(reason),
        }
    }

    // ---------------------------------------------------------------- 開いている窓（6b）

    #[tool(
        description = "開いている mdview の窓の一覧（プロセス番号・ファイルのパス・題名・未保存か・読み取り専用か・行数・自動保存を入れているか）"
    )]
    async fn list_windows(&self) -> Result<CallToolResult, ErrorData> {
        let windows = self.live_windows().await;
        let selected = self.selected.lock().ok().and_then(|s| *s);
        ok_json(json!({ "windows": windows, "selected": selected }))
    }

    #[tool(description = "以後の窓の道具が相手にする窓を選ぶ（pid は list_windows で分かる）")]
    async fn select_window(
        &self,
        Parameters(SelectRequest { pid }): Parameters<SelectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        match crate::ipc::request(pid, &json!({ "cmd": "info" })).await {
            Ok(info) => {
                if let Ok(mut selected) = self.selected.lock() {
                    *selected = Some(pid);
                }
                ok_json(json!({ "selected": info }))
            }
            Err(reason) => failed(reason),
        }
    }

    #[tool(description = "窓の本文（全文）を取る")]
    async fn get_window_text(
        &self,
        Parameters(WindowRequest { pid }): Parameters<WindowRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        match crate::ipc::request(pid, &json!({ "cmd": "get_text" })).await {
            Ok(answer) => ok_text(answer["text"].as_str().unwrap_or("").to_owned()),
            Err(reason) => failed(reason),
        }
    }

    #[tool(
        description = "窓で選んでいる文字と、その範囲（行・桁は 1 から）。何も選んでいなければ空の文字とキャレットの位置"
    )]
    async fn get_selection(
        &self,
        Parameters(WindowRequest { pid }): Parameters<WindowRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        match crate::ipc::request(pid, &json!({ "cmd": "get_selection" })).await {
            Ok(mut answer) => {
                if let Some(map) = answer.as_object_mut() {
                    map.remove("ok");
                }
                ok_json(answer)
            }
            Err(reason) => failed(reason),
        }
    }

    #[tool(
        description = "窓の本文の範囲を置き換える（行・桁は 1 から。桁は文字の数で、終わりの桁の文字の手前まで）。元に戻すで 1 回で戻る。読み取り専用の窓では断る"
    )]
    async fn replace_range(
        &self,
        Parameters(request): Parameters<ReplaceRangeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(request.pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        let message = json!({
            "cmd": "replace",
            "start": { "line": request.start_line, "column": request.start_column },
            "end": { "line": request.end_line, "column": request.end_column },
            "text": request.text,
        });
        match crate::ipc::request(pid, &message).await {
            Ok(answer) => ok_json(
                json!({ "window": pid, "changed": answer["changed"], "lines": answer["lines"] }),
            ),
            Err(reason) => failed(reason),
        }
    }

    #[tool(description = "窓の本文の位置（行・桁は 1 から）に文字を入れる。元に戻すで 1 回で戻る")]
    async fn insert_text(
        &self,
        Parameters(request): Parameters<InsertRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(request.pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        let at = json!({ "line": request.line, "column": request.column });
        let message = json!({ "cmd": "replace", "start": at, "end": at, "text": request.text });
        match crate::ipc::request(pid, &message).await {
            Ok(answer) => ok_json(
                json!({ "window": pid, "changed": answer["changed"], "lines": answer["lines"] }),
            ),
            Err(reason) => failed(reason),
        }
    }

    #[tool(
        description = "窓の本文を全文置き換える（変わった範囲だけを 1 つの編集にするので、元に戻すで 1 回で戻る）"
    )]
    async fn replace_window_text(
        &self,
        Parameters(ReplaceAllRequest { pid, text }): Parameters<ReplaceAllRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        match self.write_window(pid, &text).await {
            Ok(answer) => ok_json(
                json!({ "window": pid, "changed": answer["changed"], "lines": answer["lines"] }),
            ),
            Err(reason) => failed(reason),
        }
    }

    // ---------------------------------------------------------------- 保存・出力・窓・そのほか（6c）

    #[tool(description = "窓の文書を上書き保存する（保存先のある文書だけ）")]
    async fn save_document(
        &self,
        Parameters(WindowRequest { pid }): Parameters<WindowRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask_window(pid, json!({ "cmd": "save" })).await
    }

    #[tool(description = "窓の文書を、名前を付けて保存する（その窓の保存先も変わる）")]
    async fn save_document_as(
        &self,
        Parameters(SaveAsRequest { pid, path }): Parameters<SaveAsRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask_window(pid, json!({ "cmd": "save_as", "path": path }))
            .await
    }

    #[tool(
        description = "文書を PDF か HTML に出力する（path・text・window=true のどれかで渡す。HTML は画像ごと 1 ファイル。PDF の行の折り返しは窓から出力したものと少し違うことがある）"
    )]
    async fn export_document(
        &self,
        Parameters(ExportRequest {
            source,
            format,
            output_path,
        }): Parameters<ExportRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (text, window) = match self.read_source(&source).await {
            Ok(read) => read,
            Err(reason) => return failed(reason),
        };
        // 画像の基準と題名は、元のファイルの置き場と名前から
        let mut origin = source.path.clone().map(std::path::PathBuf::from);
        if let Some(pid) = window {
            if let Ok(info) = crate::ipc::request(pid, &json!({ "cmd": "info" })).await {
                origin = info["path"].as_str().map(std::path::PathBuf::from);
            }
        }
        let title = origin
            .as_ref()
            .and_then(|path| path.file_stem())
            .map_or_else(
                || "無題".to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            );
        let base_dir = origin
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf);
        let output = output_path.clone();
        let kind = format.trim().to_ascii_lowercase();
        let result = tokio::task::spawn_blocking(move || -> Result<usize, String> {
            struct Quiet;
            impl crate::export::ExportWatch for Quiet {}
            let document = crate::document::Document::from_text(text);
            // Marp の文書はスライドとして書き出す（v2.4.0 R-07 7d）
            let deck = crate::export::slides::deck_for(&document);
            let bytes = match (kind.as_str(), &deck) {
                ("html", Some(deck)) => crate::export::slides::html(
                    &document,
                    deck,
                    &title,
                    base_dir.as_deref(),
                    &Quiet,
                )
                .map_err(|error| error.to_string())?
                .text
                .into_bytes(),
                ("pdf", Some(deck)) => crate::export::slides::pdf(
                    &crate::export::slides::fixed_measure,
                    &document,
                    deck,
                    &title,
                    base_dir.as_deref(),
                    &Quiet,
                    crate::export::range::ExportRange::All,
                )
                .map_err(|error| error.to_string())?,
                ("pptx", Some(deck)) => crate::export::slides::pptx(
                    &crate::export::slides::fixed_measure,
                    &document,
                    deck,
                    &title,
                    base_dir.as_deref(),
                    &Quiet,
                    crate::export::range::ExportRange::All,
                    crate::io::settings::Settings::load().pptx_embed_fonts,
                )
                .map_err(|error| error.to_string())?,
                ("pptx", None) => {
                    return Err(
                        "pptx にできるのは Marp の文書（frontmatter に marp: true）だけです"
                            .to_owned(),
                    )
                }
                ("html", None) => {
                    crate::export::html::export(&document, &title, base_dir.as_deref(), &Quiet)
                        .map_err(|error| error.to_string())?
                        .text
                        .into_bytes()
                }
                ("pdf", None) => {
                    let measurer = crate::layout::measure::FixedMeasurer::default();
                    crate::export::pdf::export(
                        &document,
                        &measurer,
                        &title,
                        base_dir.as_deref(),
                        &Quiet,
                        crate::export::range::ExportRange::All,
                    )
                    .map_err(|error| error.to_string())?
                }
                (other, _) => return Err(format!("format は pdf・html・pptx です: {other}")),
            };
            std::fs::write(&output, &bytes).map_err(|error| format!("書けません: {error}"))?;
            Ok(bytes.len())
        })
        .await;
        match result {
            Ok(Ok(bytes)) => ok_json(json!({ "output_path": output_path, "bytes": bytes })),
            Ok(Err(reason)) => failed(reason),
            Err(error) => failed(error.to_string()),
        }
    }

    #[tool(description = "ファイルを mdview の新しい窓で開く")]
    async fn open_file(
        &self,
        Parameters(PathRequest { path }): Parameters<PathRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if !Path::new(&path).is_file() {
            return failed(format!("ファイルがありません: {path}"));
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => return failed(error.to_string()),
        };
        // **標準入出力は渡さない**（MCP の通り道を新しい窓に握らせない）
        match std::process::Command::new(exe)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => ok_json(json!({ "pid": child.id(), "path": path })),
            Err(error) => failed(format!("窓を開けません: {error}")),
        }
    }

    #[tool(
        description = "URL の Markdown を mdview の新しい窓で、読み取り専用で開く（GitHub・GitLab・gist の画面の URL は中身の URL に読み替える）"
    )]
    async fn open_url(
        &self,
        Parameters(UrlRequest { url }): Parameters<UrlRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if !crate::io::remote::is_url(&url) {
            return failed("http:// か https:// で始まる URL を指定してください");
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => return failed(error.to_string()),
        };
        // **標準入出力は渡さない**（MCP の通り道を新しい窓に握らせない）
        match std::process::Command::new(exe)
            .arg("--url")
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => ok_json(json!({ "pid": child.id(), "url": url })),
            Err(error) => failed(format!("窓を開けません: {error}")),
        }
    }

    #[tool(
        description = "開いている窓で、ほかのファイルを開く（いまの文書の未保存の変更は失われる）"
    )]
    async fn open_in_window(
        &self,
        Parameters(OpenInWindowRequest { pid, path }): Parameters<OpenInWindowRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if !Path::new(&path).is_file() {
            return failed(format!("ファイルがありません: {path}"));
        }
        self.ask_window(pid, json!({ "cmd": "open", "path": path }))
            .await
    }

    #[tool(description = "窓の文書を、保存してある内容に読み直す（未保存の変更は失われる）")]
    async fn reload_window(
        &self,
        Parameters(WindowRequest { pid }): Parameters<WindowRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask_window(pid, json!({ "cmd": "reload" })).await
    }

    #[tool(description = "窓を閉じる（未保存の変更は失われる）")]
    async fn close_window(
        &self,
        Parameters(WindowRequest { pid }): Parameters<WindowRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask_window(pid, json!({ "cmd": "close" })).await
    }

    #[tool(description = "mdview を終える（開いているすべての窓を閉じる。未保存の変更は失われる）")]
    async fn quit_app(&self) -> Result<CallToolResult, ErrorData> {
        let mut closed = Vec::new();
        for window in self.live_windows().await {
            let pid = window["pid"].as_u64().unwrap_or(0) as u32;
            if crate::ipc::request(pid, &json!({ "cmd": "close" }))
                .await
                .is_ok()
            {
                closed.push(pid);
            }
        }
        ok_json(json!({ "closed": closed }))
    }

    #[tool(description = "リンク（URL かファイル）を OS の既定のアプリで開く")]
    async fn open_link(
        &self,
        Parameters(LinkRequest { target }): Parameters<LinkRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (program, args) = crate::io::launch::command_for(&target);
        match std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => ok_json(json!({ "opened": target })),
            Err(error) => failed(format!("開けません: {error}")),
        }
    }

    #[tool(
        description = "mdview の設定を変える（settings.toml の鍵と値。開いている窓にもすぐ効く。mcp_ で始まる鍵は変えられない）"
    )]
    async fn set_setting(
        &self,
        Parameters(SettingRequest { key, value }): Parameters<SettingRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let key = key.trim().to_owned();
        // **MCP 自身の扱いは AI から変えさせない**（確認を許可に変えて抜けられてしまう）
        if key.starts_with("mcp_") {
            return failed("MCP の設定（mcp_ で始まる鍵）は、mdview の設定画面で変えてください");
        }
        let Some(written) = toml_value(&value) else {
            return failed("値に \" と改行は使えません");
        };
        let _lock = Settings::lock();
        let before = Settings::load();
        if before.entry(&key).is_none() && Settings::default().entry(&key).is_none() {
            return failed(format!("知らない設定の鍵です: {key}"));
        }
        let changed = Settings::from_toml(&format!("{}{key} = {written}\n", before.to_toml()));
        let after = changed.entry(&key);
        if after == before.entry(&key)
            && toml_value(after.as_deref().unwrap_or("").trim_matches('"')).as_deref()
                != Some(written.as_str())
        {
            return failed(format!("{key} にその値は使えません: {value}"));
        }
        match changed.save() {
            Ok(()) => ok_json(
                json!({ "key": key, "value": after.map(|v| v.trim_matches('"').to_owned()) }),
            ),
            Err(reason) => failed(format!("設定を保存できません: {reason}")),
        }
    }

    #[tool(description = "窓の読み取り専用を切り替える（on=true で読み取り専用、false で外す）")]
    async fn set_read_only(
        &self,
        Parameters(ReadOnlyRequest { pid, on }): Parameters<ReadOnlyRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask_window(pid, json!({ "cmd": "set_read_only", "on": on }))
            .await
    }

    #[tool(
        description = "文字をクリップボードへ写す（いまクリップボードにあるものは上書きされる）"
    )]
    async fn copy_to_clipboard(
        &self,
        Parameters(ClipboardRequest { text }): Parameters<ClipboardRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let clipboard = self.clipboard.clone();
        let count = text.chars().count();
        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut held = clipboard.lock().map_err(|error| error.to_string())?;
            // **他のプロセスが開いていたら試し直す**（Windows。窓の側と同じ考え方）
            for _ in 0..20 {
                let attempt = match held.as_mut() {
                    Some(board) => board.set_text(text.clone()),
                    None => arboard::Clipboard::new().and_then(|mut board| {
                        let result = board.set_text(text.clone());
                        *held = Some(board);
                        result
                    }),
                };
                match attempt {
                    Ok(()) => return Ok(()),
                    Err(arboard::Error::ClipboardOccupied) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            Err("クリップボードをほかのアプリが使っています".to_owned())
        })
        .await;
        match result {
            Ok(Ok(())) => ok_json(json!({ "copied": count })),
            Ok(Err(reason)) => failed(reason),
            Err(error) => failed(error.to_string()),
        }
    }

    #[tool(
        description = "窓の本文を探す（文字か正規表現。既定は大文字と小文字を区別しない）。一致した所の行・桁（1 から）と文字を返す（200 件まで）"
    )]
    async fn search_window(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let pid = match self.target(request.pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        let regex = match search_regex(
            &request.query,
            request.regex == Some(true),
            request.case_sensitive == Some(true),
        ) {
            Ok(regex) => regex,
            Err(reason) => return failed(reason),
        };
        let text = match crate::ipc::request(pid, &json!({ "cmd": "get_text" })).await {
            Ok(answer) => answer["text"].as_str().unwrap_or("").to_owned(),
            Err(reason) => return failed(reason),
        };
        let found = matches_of(&text, &regex);
        ok_json(json!({ "window": pid, "count": regex.find_iter(&text).count(), "matches": found }))
    }

    #[tool(
        description = "窓の本文の一致する所をすべて置き換える（文字か正規表現。replacement が要る）。1 つの編集なので、元に戻すで 1 回で戻る"
    )]
    async fn replace_in_window(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(replacement) = request.replacement.clone() else {
            return failed("replacement（置き換える文字）がありません");
        };
        let pid = match self.target(request.pid).await {
            Ok(pid) => pid,
            Err(reason) => return failed(reason),
        };
        let is_regex = request.regex == Some(true);
        let regex = match search_regex(
            &request.query,
            is_regex,
            request.case_sensitive == Some(true),
        ) {
            Ok(regex) => regex,
            Err(reason) => return failed(reason),
        };
        let text = match crate::ipc::request(pid, &json!({ "cmd": "get_text" })).await {
            Ok(answer) => answer["text"].as_str().unwrap_or("").to_owned(),
            Err(reason) => return failed(reason),
        };
        let count = regex.find_iter(&text).count();
        // **ただの文字なら、置き換える文字の $ をそのまま入れる**
        let replaced = if is_regex {
            regex.replace_all(&text, replacement.as_str()).into_owned()
        } else {
            regex
                .replace_all(&text, regex::NoExpand(&replacement))
                .into_owned()
        };
        match self.write_window(pid, &replaced).await {
            Ok(_) => ok_json(json!({ "window": pid, "replaced": count })),
            Err(reason) => failed(reason),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MdviewServer {
    /// **すべての道具は、呼ぶ前に操作ごとの扱いを見る**（6c）
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, ErrorData> {
        let arguments = request.arguments.clone().unwrap_or_default();
        if let Err(reason) = self.gate(&request.name, &arguments).await {
            return Ok(CallToolResult::error(vec![ContentBlock::text(reason)]).into());
        }
        let context = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(context).await
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mdview", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "mdview（Markdown エディタ）の機能を呼ぶ。文書の構造・検査・比較・パッチ・目次・見出しの番号。\
                 開いている窓の本文の読み書き・保存・出力・窓の開け閉めもできる。\
                 壊しうる操作は、利用者が mdview で許したときだけ行う（扱いは mdview の設定の「MCP」で決まる）",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 構造: 見出し・リンク・コードブロック・frontmatter・統計
    #[test]
    fn structure_reports_the_parts() {
        let text = "---\ntitle: 題\n---\n# 一\n\n[先](a.md) と <https://example.com>\n\n```rust\nfn x() {}\n```\n\n| a |\n|---|\n";
        let value = structure(text);
        assert_eq!(value["headings"][0]["title"], "一");
        assert_eq!(value["headings"][0]["line"], 4);
        assert_eq!(value["links"].as_array().unwrap().len(), 2);
        assert_eq!(value["links"][0]["target"], "a.md");
        assert_eq!(value["code_blocks"][0]["language"], "rust");
        assert_eq!(value["frontmatter"], "title: 題");
        assert_eq!(value["stats"]["tables"], 1);
        assert_eq!(
            frontmatter("+++\ntitle = \"題\"\n+++\n本文"),
            "title = \"題\""
        );
    }

    /// 本文で渡したものはそのまま、パスも本文も無ければ理由を返す
    #[test]
    fn sources_need_a_path_or_text() {
        let text = Source {
            path: None,
            text: Some("a\r\nb".to_owned()),
            window: None,
            pid: None,
        };
        assert_eq!(text.read().unwrap().0, "a\nb", "改行は \\n に揃える");
        let nothing = Source {
            path: None,
            text: None,
            window: None,
            pid: None,
        };
        assert!(nothing.read().is_err());
    }

    /// 探す: ただの文字は正規表現の記号を文字として扱い、既定は大文字小文字を区別しない
    #[test]
    fn searching_escapes_plain_text() {
        let regex = search_regex("a.b", false, false).unwrap();
        assert!(regex.is_match("A.B"));
        assert!(!regex.is_match("axb"));
        let found = matches_of("x\nA.b と a.b", &regex);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0]["line"], 2);
        assert_eq!(found[1]["column"], 7);
        assert!(search_regex("(", true, false).is_err());
    }

    /// 設定の値の書き方と、確認に出す言葉
    #[test]
    fn values_and_descriptions() {
        assert_eq!(toml_value("true").as_deref(), Some("true"));
        assert_eq!(toml_value("12").as_deref(), Some("12"));
        assert_eq!(toml_value("dark").as_deref(), Some("\"dark\""));
        assert_eq!(toml_value("a\"b"), None);
        let mut arguments = rmcp::model::JsonObject::new();
        arguments.insert("path".to_owned(), json!("C:/a.md"));
        arguments.insert("text".to_owned(), json!("あいう"));
        assert_eq!(
            describe("save_document_as", &arguments),
            "名前を付けて保存する（path: C:/a.md、text: 3 文字）"
        );
    }

    /// 道具の一覧に 28 ある（名前は MCP クライアントに見える）
    #[test]
    fn the_tools_are_listed() {
        let server = MdviewServer::new();
        let mut names: Vec<String> = server
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        names.sort();
        // **扱いの表（io::mcp_access::TOOLS）と同じ道具**（表に無い道具は既定で「確認」になってしまう）
        let mut table: Vec<String> = crate::io::mcp_access::TOOLS
            .iter()
            .map(|(name, _, _)| (*name).to_owned())
            .collect();
        table.sort();
        assert_eq!(names, table, "扱いの表と道具がずれている");
        assert_eq!(
            names,
            [
                "apply_patch",
                "close_window",
                "compare_documents",
                "copy_to_clipboard",
                "document_structure",
                "export_document",
                "get_selection",
                "get_window_text",
                "insert_text",
                "lint_markdown",
                "list_windows",
                "number_headings",
                "open_file",
                "open_in_window",
                "open_link",
                "open_url",
                "quit_app",
                "read_document",
                "reload_window",
                "replace_in_window",
                "replace_range",
                "replace_window_text",
                "save_document",
                "save_document_as",
                "search_window",
                "select_window",
                "set_read_only",
                "set_setting",
                "update_toc",
            ]
        );
    }
}
