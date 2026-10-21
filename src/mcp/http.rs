//! 手元の HTTP（v2.4.0 R-06 6d）。MCP の Streamable HTTP を **127.0.0.1 だけ**で受ける。
//!
//! 窓（GUI）の中で回す。設定「手元の HTTP で受ける」を入れた窓のうち、**ポートを取れた 1 つ**が受け、
//! ほかの窓は取れるまで待つ（受けていた窓が閉じると、次の窓が受け継ぐ）。
//!
//! **合言葉は受け始めるたびに作り直す。** 呼ぶ側は `Authorization: Bearer <合言葉>` を付ける。
//! 合言葉と口は設定フォルダの `mcp-http.json` に書く（設定画面「MCP」はここを読む）。
//! 操作の中身・許可と確認は stdio と同じ（`MdviewServer`）。

use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{json, Value};

use super::MdviewServer;
use crate::io::settings::Settings;

/// 呼ぶ側が使う道（`http://127.0.0.1:<口>/mcp`）
pub const PATH: &str = "/mcp";

/// 合言葉の長さ（バイト。16 進で倍の字数になる）
const TOKEN_BYTES: usize = 32;

/// 受けたい口（`None` なら受けない）。窓の設定が変わるたびに `want` で入れる
static WANTED: Mutex<Option<u16>> = Mutex::new(None);

/// 受け持ちの糸を起こしたか
static STARTED: std::sync::Once = std::sync::Once::new();

/// いま受けている窓の口と合言葉（`mcp-http.json` の中身）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub pid: u32,
    pub port: u16,
    pub token: String,
}

impl HostInfo {
    /// 呼ぶ側が使う URL
    pub fn url(&self) -> String {
        url_for(self.port)
    }
}

/// 口から URL を作る
pub fn url_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

/// 口と合言葉を書くファイル（設定フォルダの中）
fn info_file() -> Option<std::path::PathBuf> {
    crate::io::settings::settings_path()?
        .parent()
        .map(|dir| dir.join("mcp-http.json"))
}

/// 窓の設定を受け持ちの糸へ伝える。**初めて呼ばれたときに糸を起こす**
pub fn want(settings: &Settings) {
    let wanted = (settings.mcp_enabled && settings.mcp_http).then_some(settings.mcp_http_port);
    if let Ok(mut slot) = WANTED.lock() {
        *slot = wanted;
    }
    if wanted.is_some() {
        STARTED.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("mdview-mcp-http".to_owned())
                .spawn(host_loop);
        });
    }
}

fn wanted() -> Option<u16> {
    WANTED.lock().ok().and_then(|slot| *slot)
}

/// いま受けている窓（**その窓が開いているときだけ**）
pub fn current() -> Option<HostInfo> {
    let text = std::fs::read_to_string(info_file()?).ok()?;
    let info = parse_info(&serde_json::from_str(&text).ok()?)?;
    // 落ちた窓が残したファイルは使わない（居場所のファイルが無ければ閉じている）
    let alive = info.pid == std::process::id() || crate::ipc::registered().contains(&info.pid);
    alive.then_some(info)
}

fn parse_info(value: &Value) -> Option<HostInfo> {
    Some(HostInfo {
        pid: u32::try_from(value["pid"].as_u64()?).ok()?,
        port: u16::try_from(value["port"].as_u64()?).ok()?,
        token: value["token"].as_str()?.to_owned(),
    })
}

/// 合言葉を作る（OS の乱数。16 進）
fn new_token() -> Option<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// `Authorization` の値が合言葉と合うか。**比べる手間を字に依らず同じにする**
fn authorized(header: Option<&str>, token: &str) -> bool {
    let Some(given) = header.and_then(|value| value.strip_prefix("Bearer ")) else {
        return false;
    };
    let (given, token) = (given.trim().as_bytes(), token.as_bytes());
    given.len() == token.len()
        && given
            .iter()
            .zip(token)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// 受け持ちの糸。受けたい口がある間、取れるまで試し、取れたら受ける
fn host_loop() {
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async {
        loop {
            let Some(port) = wanted() else {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            };
            match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                Ok(listener) => serve(listener, port).await,
                // **ほかの窓（かほかのアプリ）が使っている。** 少し待って取り直す
                Err(_) => tokio::time::sleep(Duration::from_secs(3)).await,
            }
        }
    });
}

/// 取れた口で受ける。**受けたい口が変わるか、受けないことになったら止める**
async fn serve(listener: tokio::net::TcpListener, port: u16) {
    let Some(token) = new_token() else {
        return;
    };
    let pid = std::process::id();
    let Some(path) = info_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let info = json!({ "pid": pid, "port": port, "token": token, "url": url_for(port) });
    if std::fs::write(&path, info.to_string()).is_err() {
        return;
    }

    let config = StreamableHttpServerConfig::default();
    let stop = config.cancellation_token.clone();
    let service = StreamableHttpService::new(
        || Ok(MdviewServer::new()),
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    );
    let token = std::sync::Arc::new(token);
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            () = tokio::time::sleep(Duration::from_secs(1)) => {
                if wanted() != Some(port) {
                    break;
                }
                continue;
            }
        };
        let Ok((stream, _)) = accepted else {
            continue;
        };
        let service = service.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let handler = hyper::service::service_fn(move |request: hyper::Request<_>| {
                let mut service = service.clone();
                let token = token.clone();
                async move {
                    let header = request
                        .headers()
                        .get(hyper::header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok());
                    if !authorized(header, &token) {
                        return Ok::<_, std::convert::Infallible>(refuse(
                            hyper::StatusCode::UNAUTHORIZED,
                            "合言葉が違います（Authorization: Bearer <合言葉>）",
                        ));
                    }
                    if request.uri().path() != PATH {
                        return Ok(refuse(hyper::StatusCode::NOT_FOUND, "道が違います（/mcp）"));
                    }
                    tower_call(&mut service, request).await
                }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), handler)
                .await;
        });
    }
    stop.cancel();
    // **自分が書いたものだけ消す**（受け継いだ窓が書き直していたら残す）
    if current().is_some_and(|info| info.pid == pid && info.port == port) {
        let _ = std::fs::remove_file(path);
    }
}

type Body = http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>;

/// rmcp の受け口（tower の `Service`）へ渡す
async fn tower_call<S>(
    service: &mut S,
    request: hyper::Request<hyper::body::Incoming>,
) -> Result<hyper::Response<Body>, std::convert::Infallible>
where
    S: tower_service::Service<
        hyper::Request<hyper::body::Incoming>,
        Response = hyper::Response<Body>,
        Error = std::convert::Infallible,
    >,
{
    service.call(request).await
}

/// 断る答え（本文は理由の文）
fn refuse(status: hyper::StatusCode, reason: &str) -> hyper::Response<Body> {
    let mut response = hyper::Response::new(Full::new(Bytes::from(reason.to_owned())).boxed());
    *response.status_mut() = status;
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// 受けている窓が閉じるときに呼ぶ（**自分が受けていたら**ファイルを消す）
pub fn release() {
    if let Ok(mut slot) = WANTED.lock() {
        *slot = None;
    }
    if current().is_some_and(|info| info.pid == std::process::id()) {
        if let Some(path) = info_file() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_must_match_exactly() {
        let token = new_token().expect("乱数が取れる");
        assert_eq!(token.len(), TOKEN_BYTES * 2);
        assert_ne!(token, new_token().unwrap(), "毎回ちがう");
        assert!(authorized(Some(&format!("Bearer {token}")), &token));
        assert!(!authorized(Some(&token), &token), "Bearer が無い");
        assert!(!authorized(Some("Bearer "), &token));
        assert!(!authorized(
            Some(&format!("Bearer {}", &token[1..])),
            &token
        ));
        assert!(!authorized(None, &token));
    }

    #[test]
    fn the_info_file_reads_back() {
        let value = json!({ "pid": 12, "port": 39217, "token": "ab", "url": url_for(39217) });
        let info = parse_info(&value).expect("読める");
        assert_eq!(info.url(), "http://127.0.0.1:39217/mcp");
        assert!(parse_info(&json!({ "pid": 1, "port": 70000, "token": "x" })).is_none());
    }
}
