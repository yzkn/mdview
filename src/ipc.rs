//! 窓と MCP サーバーをつなぐ、手元だけの通り道（v2.4.0 R-06 6b）。
//!
//! 窓は別のプロセスで動く（R-09）。`mdview --mcp` が開いている窓の本文を読み書きするために、
//! **各窓が手元だけの通り道で待ち受ける**（Windows は名前付きパイプ、ほかの OS は UNIX ソケット）。
//! 窓は自分の居場所を、設定フォルダの `windows/<プロセス番号>.json` で知らせ、閉じるときに消す。
//! 落ちて残ったものは、MCP の側がつなげなかったときに消す。
//!
//! やりとりは **1 回の接続で 1 往復**。頼みも答えも JSON を 1 行（`{"cmd": …}` → `{"ok": true, …}`）。
//! ネットには口を開けない（名前付きパイプは手元のプロセスからしか開けない）。

use std::path::PathBuf;

use serde_json::Value;

/// 窓の居場所を置くフォルダ（設定フォルダの中）
pub fn registry_dir() -> Option<PathBuf> {
    crate::io::settings::settings_path()?
        .parent()
        .map(|dir| dir.join("windows"))
}

/// この窓の居場所のファイル
pub fn registry_file(pid: u32) -> Option<PathBuf> {
    registry_dir().map(|dir| dir.join(format!("{pid}.json")))
}

/// 通り道の名前（Windows は名前付きパイプ、ほかはソケットのパス）
pub fn endpoint(pid: u32) -> String {
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\mdview-{pid}")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join(format!("mdview-{pid}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

/// 居場所のファイルに書く中身
pub fn registry_entry(pid: u32) -> Value {
    serde_json::json!({ "pid": pid, "endpoint": endpoint(pid) })
}

/// 開いている窓（居場所のファイルがあるもの）のプロセス番号
pub fn registered() -> Vec<u32> {
    let Some(dir) = registry_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            name.strip_suffix(".json")?.parse().ok()
        })
        .collect();
    pids.sort_unstable();
    pids
}

/// 居場所のファイルを消す（閉じるとき・つなげなかったとき）
pub fn unregister(pid: u32) {
    if let Some(path) = registry_file(pid) {
        let _ = std::fs::remove_file(path);
    }
}

/// 窓へ頼みを送り、答えを受け取る（MCP の側）
pub async fn request(pid: u32, request: &Value) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let line = format!("{request}\n");
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint(pid))
        .map_err(|error| format!("窓（{pid}）につなげません: {error}"))?;
    #[cfg(not(windows))]
    let stream = tokio::net::UnixStream::connect(endpoint(pid))
        .await
        .map_err(|error| format!("窓（{pid}）につなげません: {error}"))?;

    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|error| format!("窓（{pid}）へ送れません: {error}"))?;
    writer.flush().await.map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(reader);
    let mut answer = String::new();
    reader
        .read_line(&mut answer)
        .await
        .map_err(|error| format!("窓（{pid}）から受け取れません: {error}"))?;
    let value: Value = serde_json::from_str(answer.trim())
        .map_err(|error| format!("窓（{pid}）の答えを読めません: {error}"))?;
    if value["ok"] == Value::Bool(true) {
        Ok(value)
    } else {
        Err(value["error"]
            .as_str()
            .unwrap_or("窓が断りました")
            .to_owned())
    }
}

/// ほかの窓で `path` を開いていれば、**その窓を前に出させて**プロセス番号を返す（v3 R-15）。
///
/// 自分は問い合わせない。答えない窓（落ちて居場所だけ残ったもの）は少し待って飛ばす。
/// **Windows は前に出す権利を相手へ渡しておく**（裏のプロセスは自分から前に出られない決まりがあるため）
pub async fn focus_if_open(path: &std::path::Path) -> Option<u32> {
    let me = std::process::id();
    let request = serde_json::json!({ "cmd": "focus_if_open", "path": path.to_string_lossy() });
    for pid in registered().into_iter().filter(|pid| *pid != me) {
        allow_foreground(pid);
        let asked = tokio::time::timeout(
            std::time::Duration::from_millis(800),
            self::request(pid, &request),
        )
        .await;
        if let Ok(Ok(answer)) = asked {
            if answer["open"] == Value::Bool(true) {
                return Some(pid);
            }
        }
    }
    None
}

/// 窓を出す前に使う [`focus_if_open`]（起動の引数で開くとき。v3 R-15）。**ほかの窓が無ければ何もしない**
pub fn focus_if_open_blocking(path: &std::path::Path) -> Option<u32> {
    let me = std::process::id();
    if registered().iter().all(|pid| *pid == me) {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(focus_if_open(path))
}

/// 前に出す権利を相手のプロセスへ渡す（Windows だけ。ほかの OS は要らない）
fn allow_foreground(pid: u32) {
    #[cfg(windows)]
    // SAFETY: プロセス番号を渡すだけ（失敗しても前に出ないだけで害は無い）
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
    }
    #[cfg(not(windows))]
    let _ = pid;
}

/// 同じファイルか（v3 R-15）。**実在するなら正規の形で比べる**（`a\..\b.md` と `b.md`・大文字小文字の違いを同じに）
pub fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => crate::io::positions::same_file(&a, &b),
        _ => crate::io::positions::same_file(a, b),
    }
}

/// 窓が受けた頼み。**答えは `reply` へ送る**（アプリの `update` で作る）
#[derive(Debug, Clone)]
pub struct Incoming {
    pub request: Value,
    reply: std::sync::Arc<std::sync::Mutex<Option<iced::futures::channel::oneshot::Sender<Value>>>>,
}

impl Incoming {
    /// 答える（2 度目は捨てる）
    pub fn answer(&self, value: Value) {
        if let Some(sender) = self.reply.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = sender.send(value);
        }
    }
}

/// 窓の側の待ち受け（iced の購読）。届いた頼みを `Incoming` にして流す
pub fn serve() -> impl iced::futures::Stream<Item = Incoming> {
    iced::stream::channel(16, async move |mut output| {
        use iced::futures::SinkExt;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let pid = std::process::id();
        let name = endpoint(pid);

        // 居場所を知らせる（**待ち受けを作れたら**）
        let register = || {
            if let Some(path) = registry_file(pid) {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(path, registry_entry(pid).to_string());
            }
        };

        #[cfg(windows)]
        let mut server = {
            use tokio::net::windows::named_pipe::ServerOptions;
            match ServerOptions::new()
                .first_pipe_instance(true)
                // **ネット越しの相手は受けない**
                .reject_remote_clients(true)
                .create(&name)
            {
                Ok(server) => server,
                Err(_) => return,
            }
        };
        #[cfg(not(windows))]
        let listener = {
            let _ = std::fs::remove_file(&name);
            match tokio::net::UnixListener::bind(&name) {
                Ok(listener) => listener,
                Err(_) => return,
            }
        };
        register();

        loop {
            #[cfg(windows)]
            let stream = {
                use tokio::net::windows::named_pipe::ServerOptions;
                if server.connect().await.is_err() {
                    continue;
                }
                // **次の相手のために、新しい口を先に作る**（つないだものは、この 1 往復に使う）
                let next = match ServerOptions::new()
                    .reject_remote_clients(true)
                    .create(&name)
                {
                    Ok(next) => next,
                    Err(_) => return,
                };
                std::mem::replace(&mut server, next)
            };
            #[cfg(not(windows))]
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            };

            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            if reader.read_line(&mut line).await.is_err() {
                continue;
            }
            let answer = match serde_json::from_str::<Value>(line.trim()) {
                Ok(request) => {
                    let (sender, receiver) = iced::futures::channel::oneshot::channel();
                    let incoming = Incoming {
                        request,
                        reply: std::sync::Arc::new(std::sync::Mutex::new(Some(sender))),
                    };
                    if output.send(incoming).await.is_err() {
                        return;
                    }
                    receiver.await.unwrap_or_else(
                        |_| serde_json::json!({ "ok": false, "error": "窓が答えませんでした" }),
                    )
                }
                Err(error) => {
                    serde_json::json!({ "ok": false, "error": format!("頼みを読めません: {error}") })
                }
            };
            let _ = writer.write_all(format!("{answer}\n").as_bytes()).await;
            let _ = writer.flush().await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 通り道の名前と居場所のファイルは、プロセス番号ごとに決まる
    #[test]
    fn endpoints_follow_the_process() {
        assert!(endpoint(42).ends_with("mdview-42") || endpoint(42).ends_with("mdview-42.sock"));
        assert_eq!(registry_entry(7)["pid"], 7);
        if let Some(path) = registry_file(7) {
            assert!(path.ends_with("windows/7.json") || path.ends_with("windows\\7.json"));
        }
    }

    /// 同じファイルは、書き方（`..`・大文字小文字）が違っても同じ（v3 R-15）
    #[test]
    fn the_same_file_matches_however_it_is_written() {
        let dir = std::env::temp_dir().join(format!("mdview-same-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let file = dir.join("Doc.md");
        std::fs::write(&file, "x").unwrap();
        assert!(same_path(&file, &dir.join("sub").join("..").join("Doc.md")));
        assert_eq!(
            same_path(&file, &dir.join("doc.md")),
            cfg!(any(windows, target_os = "macos"))
        );
        assert!(!same_path(&file, &dir.join("other.md")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
