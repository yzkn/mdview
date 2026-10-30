//! 起動時に開くファイルを、窓と GPU の準備と並べて読む（v2.2.2）。
//!
//! iced は `App::new` を、窓を作って GPU の準備（数百 ms）をするより**前に、同じ糸で**呼ぶ
//! （`iced_winit` の `run`）。そこでファイルを読んで解析すると、その時間がそのまま起動の
//! 待ち時間に足される（10MB で約 190 ms）。
//!
//! そこで `main()` の最初に別の糸で読み始め、`App::new` は**待たずに**空の本文で窓を出す。
//! 出来上がった文書は `Message::Preloaded` で受け取って差し替える。GPU の準備のほうが長いので、
//! 最初の画面にはふつう中身が出る。
//!
//! **差し替わるまでは、本文の書き換えと上書き保存を断る**（空の本文で元のファイルを
//! 上書きしない）。試験の道具への「話せる」と `-g` も、差し替えてから行う。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use iced::futures::channel::oneshot;

use crate::document::Document;
use crate::io::LoadedFile;

/// 読んだ結果。`LoadedFile::text` は文書へ移したので空
pub type Outcome = Result<(LoadedFile, Document), String>;

/// `Message` に載せる包み。**`Document` は `Debug` を持たない**ので、中身を隠して運ぶ
#[derive(Clone)]
pub struct Preloaded(pub Arc<Mutex<Option<Outcome>>>);

impl Preloaded {
    pub fn new(outcome: Outcome) -> Self {
        Self(Arc::new(Mutex::new(Some(outcome))))
    }

    /// 中身を取り出す（1 度だけ）
    pub fn take(&self) -> Option<Outcome> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

impl std::fmt::Debug for Preloaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Preloaded")
    }
}

/// 読み始めたもの
struct Started {
    path: PathBuf,
    receiver: Mutex<Option<oneshot::Receiver<Outcome>>>,
}

static STARTED: OnceLock<Started> = OnceLock::new();

/// 起動引数の 1 つ目のファイルを、別の糸で読み始める。**`main()` で窓を作る前に呼ぶ**
pub fn start() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch = super::window::parse_launch(&args);
    let Some(path) = launch.files.first().cloned() else {
        return;
    };
    let (sender, receiver) = oneshot::channel();
    let target = path.clone();
    let spawned = std::thread::Builder::new()
        .name("preload".to_owned())
        .spawn(move || {
            // **大きすぎれば先頭だけ**（v3 R-12）。設定はここで読む（窓より先に始まるため）
            let limit = crate::io::settings::Settings::load().large_file_mb;
            crate::io::set_size_limit(u64::from(limit) * 1024 * 1024);
            let outcome = crate::io::load_limited(&target, None)
                .map(|mut file| {
                    let text = std::mem::take(&mut file.text);
                    let plain = super::reads_as_plain(Some(&file.path));
                    (file, Document::plain_text(text, plain))
                })
                .map_err(|error| format!("{error}"));
            let _ = sender.send(outcome);
        });
    // **糸を作れなければ読み始めない**（`App::new` がこれまでどおり同期で読む）
    if spawned.is_ok() {
        let _ = STARTED.set(Started {
            path,
            receiver: Mutex::new(Some(receiver)),
        });
    }
}

/// そのファイルを読みかけていれば、受け口を渡す（1 度だけ）
pub fn take(path: &Path) -> Option<oneshot::Receiver<Outcome>> {
    let started = STARTED.get()?;
    if started.path != path {
        return None;
    }
    started.receiver.lock().ok()?.take()
}
