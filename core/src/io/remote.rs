//! URL から読む（v2.4.0 R-09）。**利用者が URL を指したときだけ**ネットへ取りに行く。
//!
//! - 読めるのは `http://` と `https://` だけ
//! - GitHub・GitLab・gist の画面の URL は、中身（raw）の URL に読み替える
//! - 大きすぎるもの（`TEXT_LIMIT`・`IMAGE_LIMIT` 超）は読まない。待つのは `TIMEOUT` まで
//! - 証明書は OS の確かめ方に任せる（業務の中継が差し込む証明書も、OS が信じていれば通る）
//! - 中継（プロキシ）は環境変数 `HTTPS_PROXY`・`HTTP_PROXY`・`NO_PROXY` に従い、無ければ（設定で入れていれば）
//!   **OS に聞く**（Windows・macOS は自動構成スクリプト・自動検出・手動の設定のどれでも。Linux は GNOME の手動の設定。
//!   v2.6.0 R-02・v2.7.0 R-04。`super::system_proxy`）

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ureq::ResponseExt;

/// 文書として読む上限
pub const TEXT_LIMIT: u64 = 20 * 1024 * 1024;
/// 画像として読む上限
pub const IMAGE_LIMIT: u64 = 20 * 1024 * 1024;
/// 待つ長さ（つなぐ・受け取るの合わせて）
const TIMEOUT: Duration = Duration::from_secs(20);

/// 中継先を OS に聞くか（設定。v2.6.0 R-02）
static SYSTEM_PROXY: AtomicBool = AtomicBool::new(true);

/// 中継先を OS に聞くか（アプリが設定に合わせて呼ぶ）
pub fn use_system_proxy(on: bool) {
    SYSTEM_PROXY.store(on, Ordering::Relaxed);
}

/// つなぐときの中継先と、その名前（知らせに出す）。直接つなぐなら `(None, None)`
fn proxy_for(url: &str) -> (Option<ureq::Proxy>, Option<String>) {
    // 環境変数があれば、それを先に使う（いままでと同じ）
    if let Some(proxy) = ureq::Proxy::try_from_env() {
        return (Some(proxy), Some("環境変数の中継".to_owned()));
    }
    if !SYSTEM_PROXY.load(Ordering::Relaxed) {
        return (None, None);
    }
    // **相手の名前ごとに覚える。** 自動構成スクリプトは取ってきて評価するので、画像ごとに聞くと遅い
    type Cache = Mutex<HashMap<(bool, String), Option<String>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (
        url.trim().to_ascii_lowercase().starts_with("https://"),
        super::system_proxy::host_of(url),
    );
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let known = cache.lock().ok().and_then(|map| map.get(&key).cloned());
    let address = match known {
        Some(address) => address,
        None => {
            let address = super::system_proxy::proxy_for(url);
            if let Ok(mut map) = cache.lock() {
                map.insert(key, address.clone());
            }
            address
        }
    };
    match address.and_then(|address| Some((ureq::Proxy::new(&address).ok()?, address))) {
        Some((proxy, address)) => (Some(proxy), Some(format!("OS の設定の中継 {address}"))),
        None => (None, None),
    }
}

/// URL として読めるか（`http://`・`https://` で始まる）
pub fn is_url(text: &str) -> bool {
    let lower = text.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// 画面の URL を中身の URL に読み替える（GitHub・GitLab・gist）。読み替えなければそのまま
pub fn normalize(url: &str) -> String {
    let url = url.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host_lower = host.to_ascii_lowercase();
    // 断片（#…）は外す。問い合わせ（?…）は残す
    let path = path.split('#').next().unwrap_or("");
    match host_lower.as_str() {
        // github.com/<owner>/<repo>/blob/<ref>/<path> → raw.githubusercontent.com/<owner>/<repo>/<ref>/<path>
        "github.com" | "www.github.com" => {
            let parts: Vec<&str> = path.splitn(4, '/').collect();
            if parts.len() == 4 && (parts[2] == "blob" || parts[2] == "raw") {
                let (owner, repo, rest) = (parts[0], parts[1], parts[3]);
                let rest = rest.split('?').next().unwrap_or(rest);
                return format!("{scheme}://raw.githubusercontent.com/{owner}/{repo}/{rest}");
            }
            url.to_owned()
        }
        // gist.github.com/<user>/<id> → gist.githubusercontent.com/<user>/<id>/raw
        "gist.github.com" => {
            let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
            if parts.len() == 2 && !parts[1].is_empty() {
                return format!(
                    "{scheme}://gist.githubusercontent.com/{}/{}/raw",
                    parts[0], parts[1]
                );
            }
            url.to_owned()
        }
        // GitLab（gitlab.com と、gitlab を名に含む自前の置き場）: /-/blob/ → /-/raw/
        _ if host_lower.contains("gitlab") && path.contains("/-/blob/") => {
            format!(
                "{scheme}://{host}/{}",
                path.replacen("/-/blob/", "/-/raw/", 1)
            )
        }
        _ => url.to_owned(),
    }
}

/// 文書の URL から、相対の参照を URL にする（画像。`./a.png`・`../img/b.png`・`/x.png`）
pub fn join(base: &str, reference: &str) -> Option<String> {
    if is_url(reference) {
        return Some(reference.to_owned());
    }
    if reference.starts_with("data:") || reference.starts_with("//") {
        return None;
    }
    let (scheme, rest) = base.split_once("://")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or("");
    if let Some(absolute) = reference.strip_prefix('/') {
        return Some(format!("{scheme}://{host}/{absolute}"));
    }
    // 文書の置き場（最後の / まで）から辿る
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for piece in reference.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    Some(format!("{scheme}://{host}/{}", parts.join("/")))
}

/// URL の最後の名前（題名に使う。`a/b/c.md?x` → `c.md`）
pub fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if name.is_empty() || name.contains(':') {
        url.to_owned()
    } else {
        name.to_owned()
    }
}

/// 取ってくる（上限 `limit` バイト）。返すのは中身と、行き着いた URL
pub fn fetch(url: &str, limit: u64) -> Result<(Vec<u8>, String), String> {
    if !is_url(url) {
        return Err("http:// か https:// で始まる URL を指定してください".to_owned());
    }
    let (proxy, via) = proxy_for(url);
    // つなげなかったときは、どの中継を使ったかも出す
    let describe = |error: &ureq::Error| match &via {
        Some(via) => format!("{}（{via}）", describe(error)),
        None => describe(error),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .user_agent(concat!("mdview/", env!("CARGO_PKG_VERSION")))
        .proxy(proxy)
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|error| describe(&error))?;
    let reached = response.get_uri().to_string();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|error| match error {
            ureq::Error::BodyExceedsLimit(_) => {
                format!("大きすぎます（{} MB まで）", limit / 1024 / 1024)
            }
            other => describe(&other),
        })?;
    Ok((bytes, reached))
}

/// 失敗の理由を、利用者に分かる言葉にする
fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => format!("相手が断りました（HTTP {code}）"),
        ureq::Error::Timeout(_) => "時間内に返事がありません".to_owned(),
        ureq::Error::HostNotFound => {
            "相手の名前が見つかりません（URL か、ネットへのつながりを確かめてください）".to_owned()
        }
        other => format!("読めません: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosting_pages_become_raw() {
        assert_eq!(
            normalize("https://github.com/owner/repo/blob/main/docs/a.md"),
            "https://raw.githubusercontent.com/owner/repo/main/docs/a.md"
        );
        assert_eq!(
            normalize("https://github.com/owner/repo/blob/v1.0/README.md?plain=1#L3"),
            "https://raw.githubusercontent.com/owner/repo/v1.0/README.md"
        );
        assert_eq!(
            normalize("https://gist.github.com/user/0123abcd"),
            "https://gist.githubusercontent.com/user/0123abcd/raw"
        );
        assert_eq!(
            normalize("https://gitlab.com/group/project/-/blob/main/README.md"),
            "https://gitlab.com/group/project/-/raw/main/README.md"
        );
        // 読み替えないもの
        let plain = "https://example.com/a/b.md";
        assert_eq!(normalize(plain), plain);
        assert_eq!(
            normalize("https://github.com/owner/repo"),
            "https://github.com/owner/repo"
        );
    }

    #[test]
    fn relative_references_follow_the_document() {
        let base = "https://example.com/docs/guide/intro.md?x=1";
        assert_eq!(
            join(base, "img/a.png").as_deref(),
            Some("https://example.com/docs/guide/img/a.png")
        );
        assert_eq!(
            join(base, "./a.png").as_deref(),
            Some("https://example.com/docs/guide/a.png")
        );
        assert_eq!(
            join(base, "../b.png").as_deref(),
            Some("https://example.com/docs/b.png")
        );
        assert_eq!(
            join(base, "/root.png").as_deref(),
            Some("https://example.com/root.png")
        );
        assert_eq!(
            join(base, "https://cdn.example.com/c.png").as_deref(),
            Some("https://cdn.example.com/c.png")
        );
        assert_eq!(join(base, "data:image/png;base64,AA"), None);
    }

    #[test]
    fn names_and_schemes() {
        assert_eq!(file_name("https://example.com/a/b/c.md?x#y"), "c.md");
        assert!(is_url(" HTTPS://example.com"));
        assert!(!is_url("file:///c:/a.md"));
        assert!(fetch("ftp://example.com/a.md", 10).is_err());
    }
}
