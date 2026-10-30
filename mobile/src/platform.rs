//! 端末の書類を開く・書き戻す（v3 R-02）。
//!
//! 書類は `Handle`（Android は `content://` の URI、iOS は書類の印、手元はファイルのパス）で指す。
//! 中身はバイト列のまま渡し、文字コード・改行は土台（`io::from_bytes`・`io::encode_for_save`）が決める。
//!
//! **書類の選択は待たない**（`pick_*` は始めるだけ。選ばれたら `poll` で受け取る）。iOS は画面の糸の上で
//! 動くので、待つと止まる。

/// 書類を指すもの
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handle(pub String);

/// 開いた書類
#[derive(Debug, Clone)]
pub struct Opened {
    pub name: String,
    pub handle: Handle,
    pub bytes: Vec<u8>,
}

/// 選択の結果
#[derive(Debug, Clone)]
pub enum Picked {
    /// まだ選んでいる
    Waiting,
    /// 選ばれなかった
    Cancelled,
    Opened(Opened),
    /// 出力の先が決まり、書いた
    Exported(String),
    Failed(String),
}

/// 出力の形
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Html,
}

impl Format {
    pub fn mime(self) -> &'static str {
        match self {
            Format::Pdf => "application/pdf",
            Format::Html => "text/html",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Html => "html",
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::*;

/// 手元で動きを見るとき（Windows など）。開くのは起動の引数のファイルだけ
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop {
    use super::*;

    /// 起動の引数で渡されたファイル
    pub fn initial() -> Option<Opened> {
        let path = std::env::args().nth(1)?;
        let bytes = std::fs::read(&path).ok()?;
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        Some(Opened {
            name,
            handle: Handle(path),
            bytes,
        })
    }

    pub fn pick_open() -> Result<(), String> {
        Err("手元の確認では、開くファイルを起動の引数で渡します".into())
    }

    pub fn pick_export(_name: &str, _format: Format, _bytes: Vec<u8>) -> Result<(), String> {
        Err("手元の確認では出力しません".into())
    }

    pub fn share(_name: &str, _format: Format, _bytes: Vec<u8>) -> Result<(), String> {
        Err("手元の確認では共有しません".into())
    }

    pub fn open_link(_href: &str) -> Result<(), String> {
        Ok(())
    }

    pub fn poll() -> Picked {
        Picked::Waiting
    }

    pub fn write(handle: &Handle, bytes: &[u8]) -> Result<(), String> {
        std::fs::write(&handle.0, bytes).map_err(|error| error.to_string())
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub use native::*;

/// Android・iOS（端末の部品。src/native.rs）
#[cfg(any(target_os = "android", target_os = "ios"))]
mod native {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde::Deserialize;

    use super::*;
    use crate::native::{self as parts, plugin};

    pub fn initial() -> Option<Opened> {
        None
    }

    /// 一時のファイルの道（書くたびに別の名前）
    fn scratch(stem: &str) -> Result<PathBuf, String> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = parts::text(parts::cache_dir(plugin()?))?;
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(PathBuf::from(dir).join(format!("{stem}-{number}")))
    }

    fn put(stem: &str, bytes: &[u8]) -> Result<String, String> {
        let path = scratch(stem)?;
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// 出力・共有の頼み（JSON）
    fn request(name: &str, format: Format, path: &str) -> String {
        serde_json::json!({ "name": name, "mime": format.mime(), "path": path }).to_string()
    }

    pub fn pick_open() -> Result<(), String> {
        parts::ok(parts::pick_open(plugin()?))
    }

    pub fn pick_export(name: &str, format: Format, bytes: Vec<u8>) -> Result<(), String> {
        let path = put("export", &bytes)?;
        parts::ok(parts::pick_export(plugin()?, request(name, format, &path)))
    }

    pub fn share(name: &str, format: Format, bytes: Vec<u8>) -> Result<(), String> {
        let path = put("share", &bytes)?;
        parts::ok(parts::share(plugin()?, request(name, format, &path)))
    }

    pub fn open_link(href: &str) -> Result<(), String> {
        parts::ok(parts::open_link(plugin()?, href.to_owned()))
    }

    pub fn write(handle: &Handle, bytes: &[u8]) -> Result<(), String> {
        let path = put("save", bytes)?;
        let request = serde_json::json!({ "uri": handle.0, "path": path }).to_string();
        let result = parts::ok(parts::write_back(plugin()?, request));
        let _ = std::fs::remove_file(&path);
        result
    }

    #[derive(Deserialize)]
    struct Reply {
        kind: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        uri: String,
        #[serde(default)]
        path: String,
        #[serde(default)]
        error: String,
    }

    pub fn poll() -> Picked {
        let reply = match plugin().and_then(|plugin| parts::text(parts::poll(plugin))) {
            Ok(reply) if reply.is_empty() => return Picked::Waiting,
            Ok(reply) => reply,
            Err(error) => return Picked::Failed(error),
        };
        let Ok(reply) = serde_json::from_str::<Reply>(&reply) else {
            return Picked::Failed(format!("端末の部品の返事を読めません: {reply}"));
        };
        match reply.kind.as_str() {
            "opened" => match std::fs::read(&reply.path) {
                Ok(bytes) => {
                    let _ = std::fs::remove_file(&reply.path);
                    Picked::Opened(Opened {
                        name: reply.name,
                        handle: Handle(reply.uri),
                        bytes,
                    })
                }
                Err(error) => Picked::Failed(error.to_string()),
            },
            "exported" => Picked::Exported(reply.name),
            "cancelled" | "shared" => Picked::Cancelled,
            _ => Picked::Failed(reply.error),
        }
    }
}
