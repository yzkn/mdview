//! 異常終了時の退避と、次回起動時の復帰（§18.3）。
//!
//! **パニックを捕まえて、未保存の本文を書き出してから落ちる。**
//!
//! v1 は描画を WebView2 が担っており、描画側の異常がアプリ本体を
//! 巻き込みにくかった。v2 は描画を自前で持つため、クラッシュで編集内容を
//! 失う可能性が上がっている。
//!
//! # なぜ「控え」を持つのか
//!
//! パニックの受け口（`panic::set_hook`）は、落ちた糸の上で動く。
//! そこから画面の状態（`App`）へは手が届かない。**だから、編集のたびに
//! 控えを置いておき、受け口はそれを書き出すだけにする。**
//!
//! 控えを置くのは安い。`Rope` の複製は木を共有するので、10MB の文書でも
//! 写しが起きない（§12.1）。
//!
//! # 置き場
//!
//! OS の一時フォルダの下。**設定と同じ場所には置かない**——消し忘れても
//! 一時フォルダは OS が掃除する。
//!
//! | ファイル | 中身 |
//! |---|---|
//! | `draft.md` | 本文（UTF-8 のまま） |
//! | `draft.path` | 元のファイルのパス（新規なら空） |
//!
//! **1 つのファイルに詰めない。** 区切りを決めると、本文にその区切りが
//! 入ったときに壊れる。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ropey::Rope;

/// 退避したもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// 元のファイル。新規文書なら `None`
    pub path: Option<PathBuf>,
    pub text: String,
}

impl Draft {
    /// 画面に出す名前。
    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "無題".to_owned())
    }
}

/// いま編集中のもの（受け口が書き出す）。
///
/// **`None` なら書き出さない。** 保存した直後や、開いた直後がそれにあたる
static PENDING: Mutex<Option<(Option<PathBuf>, Rope)>> = Mutex::new(None);

/// 退避先のフォルダ。
///
/// **差し替えられるようにしてある。** 受け口（`arm`）まで含めて試験するには、
/// 本物の置き場を汚さずに動かせる必要がある。
/// 一時フォルダが使えない環境で逃がす口にもなる。
pub const FOLDER_ENV: &str = "MDVIEW_DRAFT_DIR";

fn folder() -> PathBuf {
    std::env::var_os(FOLDER_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mdview"))
}

fn body_path(dir: &Path) -> PathBuf {
    dir.join("draft.md")
}

fn origin_path(dir: &Path) -> PathBuf {
    dir.join("draft.path")
}

/// 編集中の内容を控える（§18.3）。
///
/// **保存されていないときだけ呼ぶ。** 保存済みのものを控えても、
/// 次回起動時に「復元しますか」と聞く材料が増えるだけである。
pub fn remember(text: &Rope, path: Option<&Path>) {
    // **受け口の中で詰まらせない。** 握れなければ諦める（控えが古くなるだけ）
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some((path.map(Path::to_path_buf), text.clone()));
    }
}

/// いまの控えを**ディスクへ書く**（§18.3）。
///
/// **パニックを待たない。** タスクマネージャーからの終了・電源落ち・
/// ブルースクリーンでは、プロセスの中のコードが 1 行も動かない
/// （`TerminateProcess` は受け口を呼ばない）。書いておかないと戻せない。
///
/// 戻り値は「書いたか」。控えが無ければ書かない。
pub fn flush() -> bool {
    let Ok(pending) = PENDING.lock() else {
        return false;
    };
    let Some((path, text)) = pending.as_ref() else {
        return false;
    };
    save_in(&folder(), path.as_deref(), &text.to_string()).is_ok()
}

/// 控えを捨てる（保存した・閉じた）。
pub fn forget() {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = None;
    }
    let _ = clear_in(&folder());
}

/// パニックの受け口を仕掛ける（`main` の最初で 1 度だけ）。
///
/// **もとの受け口も呼ぶ。** 置き換えてしまうと、落ちた理由が出なくなる
pub fn arm() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_pending();
        previous(info);
    }));
}

/// 控えがあれば書き出す。
///
/// **ここで失敗しても何もしない。** すでに落ちている最中であり、
/// 報告する先が無い
fn write_pending() {
    let Ok(pending) = PENDING.lock() else {
        return;
    };
    let Some((path, text)) = pending.as_ref() else {
        return;
    };
    let _ = save_in(&folder(), path.as_deref(), &text.to_string());
}

/// 退避したものがあれば読む。**読んでも消さない**（答えを聞くまで残す）。
pub fn pending() -> Option<Draft> {
    load_in(&folder())
}

/// 退避したものを捨てる。
pub fn discard() {
    let _ = clear_in(&folder());
}

// ---------------------------------------------------------------- 置き場を渡す形
//
// **試験のために分ける。** 一時フォルダを汚さずに確かめられる

pub fn save_in(dir: &Path, path: Option<&Path>, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(body_path(dir), text)?;
    let origin = path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::fs::write(origin_path(dir), origin)?;
    Ok(())
}

pub fn load_in(dir: &Path) -> Option<Draft> {
    let text = std::fs::read_to_string(body_path(dir)).ok()?;
    // **本文が空なら復元するものが無い。** 聞くだけ無駄である
    if text.is_empty() {
        return None;
    }
    let origin = std::fs::read_to_string(origin_path(dir)).unwrap_or_default();
    Some(Draft {
        path: (!origin.is_empty()).then(|| PathBuf::from(origin)),
        text,
    })
}

pub fn clear_in(dir: &Path) -> std::io::Result<()> {
    for path in [body_path(dir), origin_path(dir)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **置き場を差し替える試験どうしを順番に走らせる。**
    ///
    /// `MDVIEW_DRAFT_DIR` はプロセス全体のもので、並行して走らせると
    /// 一方が他方の置き場を消す。実際に落ちた（2026-10-05）。
    ///
    /// `--test-threads=1` に頼らない。**試験の側で閉じる。**
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// 鍵を握る。前の試験が落ちていても握れるようにする。
    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdview-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 書いて読むと元に戻る。
    #[test]
    fn a_draft_round_trips() {
        let dir = temp("round");
        let origin = PathBuf::from("C:/docs/a.md");
        save_in(&dir, Some(&origin), "本文です。\n2 行目").expect("書ける");

        let draft = load_in(&dir).expect("読める");
        assert_eq!(draft.text, "本文です。\n2 行目");
        assert_eq!(draft.path, Some(origin));
    }

    /// 新規文書（保存先が無い）でも退避できる。
    #[test]
    fn an_unsaved_document_has_no_origin() {
        let dir = temp("new");
        save_in(&dir, None, "書きかけ").expect("書ける");

        let draft = load_in(&dir).expect("読める");
        assert_eq!(draft.path, None);
        assert_eq!(draft.name(), "無題");
    }

    /// **本文に何が入っていても壊れない。** 区切りを決めていないため
    #[test]
    fn the_body_can_contain_anything() {
        let dir = temp("body");
        let tricky = "---\npath: 嘘のパス\n---\n\n改行も\r\nタブ\tも";
        save_in(&dir, Some(Path::new("C:/a.md")), tricky).expect("書ける");

        assert_eq!(load_in(&dir).expect("読める").text, tricky);
    }

    /// 捨てたら読めなくなる。
    #[test]
    fn discarding_removes_it() {
        let dir = temp("clear");
        save_in(&dir, None, "書きかけ").expect("書ける");
        assert!(load_in(&dir).is_some());

        clear_in(&dir).expect("消せる");
        assert!(load_in(&dir).is_none());
    }

    /// **無いものを捨てても失敗しない。** 正常終了のたびに呼ぶため
    #[test]
    fn clearing_nothing_is_fine() {
        let dir = temp("nothing");
        clear_in(&dir).expect("失敗しない");
        clear_in(&dir).expect("2 度目も失敗しない");
    }

    /// 空の本文は復元の対象にしない。
    #[test]
    fn an_empty_draft_is_not_offered() {
        let dir = temp("empty");
        save_in(&dir, None, "").expect("書ける");
        assert_eq!(load_in(&dir), None);
    }

    /// 置き場が無くても読もうとして落ちない。
    #[test]
    fn a_missing_folder_is_safe() {
        assert_eq!(load_in(Path::new("C:/存在しないはずの場所/mdview")), None);
    }

    /// **落ちたら本当に書き出される**（§18.3 / 受入条件 A2-07）。
    ///
    /// 受け口（`arm`）を仕掛けたうえで実際にパニックさせる。
    /// 書き出しは落ちる途中で起きるので、**捕まえずに落とすところまで
    /// 通さないと確かめたことにならない**（`catch_unwind` は巻き戻しを
    /// 止めるだけで、受け口はその前に動く）。
    #[test]
    fn a_panic_writes_the_draft() {
        let _guard = lock_env();
        let dir = temp("panic");
        // **本物の置き場を汚さない**
        std::env::set_var(FOLDER_ENV, &dir);

        arm();
        remember(
            &Rope::from_str("書きかけの本文\n2 行目"),
            Some(Path::new("C:/docs/もとの文書.md")),
        );

        // わざと落とす。巻き戻しはここで止める
        let fell = std::panic::catch_unwind(|| panic!("わざと落とす（§18.3 の確認）"));
        assert!(fell.is_err(), "落ちていない（この試験の前提が崩れている）");

        let draft = load_in(&dir).expect("**退避されていない**（A2-07 を満たさない）");
        assert_eq!(draft.text, "書きかけの本文\n2 行目");
        assert_eq!(draft.name(), "もとの文書.md");

        forget();
        std::env::remove_var(FOLDER_ENV);
    }

    /// **保存したら控えは消える。** 残すと、次に開いたとき身に覚えのない
    /// 復元を聞かれる
    #[test]
    fn forgetting_clears_the_draft() {
        let _guard = lock_env();
        let dir = temp("forget");
        std::env::set_var(FOLDER_ENV, &dir);

        remember(&Rope::from_str("書きかけ"), None);
        save_in(&dir, None, "書きかけ").expect("書ける");
        assert!(pending().is_some());

        forget();
        assert!(pending().is_none(), "控えが残っている");

        std::env::remove_var(FOLDER_ENV);
    }

    /// 画面に出す名前はファイル名だけ。
    #[test]
    fn the_name_is_just_the_file_name() {
        let draft = Draft {
            path: Some(PathBuf::from("C:/とても/長い/道のり/報告書.md")),
            text: String::new(),
        };
        assert_eq!(draft.name(), "報告書.md");
    }
}
