//! 権限の要る場所への保存（v3 R-14）。
//!
//! 保存が「権限が無い」で失敗したら、本文を一時ファイルへ書き、**OS の確認を経て**元の場所へ写す。
//!
//! - Windows: 自分の実行ファイルを UAC で上げて起こし（`--elevated-write 元 先`）、中身を書かせる
//!   （`cmd` の `copy` を使わないのは、パスの `%` などを解かれるため）
//! - macOS: `osascript` の `with administrator privileges` で `cp`
//! - Linux など: `pkexec cp`
//!
//! **書き込み先の権限・持ち主は変えない**（ファイルを作り直さず、中身だけを書く。`cp` も既にあるファイルへは中身を書く）

use std::path::Path;

/// 上げた権限で起こされたときの引数
pub const ELEVATED_WRITE: &str = "--elevated-write";

/// 上げた権限で起こされた側: `from` の中身を `to` へ書く。終わりの番号を返す（0 なら書けた）
pub fn write_as_helper(from: &Path, to: &Path) -> i32 {
    match std::fs::read(from).and_then(|bytes| std::fs::write(to, bytes)) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// 上げた権限で `from` の中身を `to` へ写す。**OS の確認を断ったら `Err`**（理由を添える）
pub fn copy_elevated(from: &Path, to: &Path) -> Result<(), String> {
    imp::copy_elevated(from, to)
}

/// Windows のコマンド行の 1 つの引数として囲む（MSVC の決まり。閉じる `"` の前の `\` は倍にする）
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_windows(argument: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0usize;
    for ch in argument.chars() {
        match ch {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                out.push('"');
                slashes = 0;
                continue;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', slashes));
                slashes = 0;
                out.push(ch);
                continue;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn copy_elevated(from: &Path, to: &Path) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let parameters = format!(
            "{} {} {}",
            super::ELEVATED_WRITE,
            super::quote_windows(&from.to_string_lossy()),
            super::quote_windows(&to.to_string_lossy()),
        );
        let verb = wide("runas".as_ref());
        let file = wide(exe.as_os_str());
        let parameters = wide(parameters.as_ref());
        // SAFETY: 構造体は 0 で埋めてから要る欄だけを入れる（Win32 の決まり）。文字列は呼ぶ間だけ生きていればよい
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        // 窓は出さない（書いたらすぐ終わる）
        info.nShow = 0;
        // SAFETY: info は上で正しく埋めた
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            // SAFETY: 直前の失敗の理由を引くだけ
            let error = unsafe { GetLastError() };
            return Err(if error == ERROR_CANCELLED {
                "管理者の権限を断りました".to_owned()
            } else {
                format!("管理者の権限で起こせません（{error}）")
            });
        }
        let process = info.hProcess;
        if process.is_null() {
            return Err("管理者の権限で起こせません".to_owned());
        }
        let mut code = 1u32;
        // SAFETY: 起こしたプロセスの取っ手を待ち、終わりの番号を引いて閉じる
        unsafe {
            WaitForSingleObject(process, INFINITE);
            GetExitCodeProcess(process, &mut code);
            CloseHandle(process);
        }
        if code == 0 {
            Ok(())
        } else {
            Err("管理者の権限でも書けませんでした".to_owned())
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::Path;

    pub fn copy_elevated(from: &Path, to: &Path) -> Result<(), String> {
        // パスは引数で渡す（台本の文字に混ぜない）
        let output = std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "do shell script \"cp \" & quoted form of item 1 of argv & \" \" & quoted form of item 2 of argv with administrator privileges",
                "-e",
                "end run",
            ])
            .arg(from)
            .arg(to)
            .output()
            .map_err(|error| format!("osascript を起こせません: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let reason = String::from_utf8_lossy(&output.stderr);
        Err(if reason.contains("-128") {
            "管理者の権限を断りました".to_owned()
        } else {
            format!("管理者の権限でも書けませんでした: {}", reason.trim())
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::path::Path;

    pub fn copy_elevated(from: &Path, to: &Path) -> Result<(), String> {
        let status = std::process::Command::new("pkexec")
            .arg("cp")
            .arg("--")
            .arg(from)
            .arg(to)
            .status()
            .map_err(|error| {
                format!("pkexec を起こせません（入っていないかもしれません）: {error}")
            })?;
        match status.code() {
            Some(0) => Ok(()),
            // 126: 確認を断った・127: 認証できなかった（pkexec の決まり）
            Some(126) | Some(127) => Err("管理者の権限を断りました".to_owned()),
            _ => Err("管理者の権限でも書けませんでした".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows の引数の囲み方（空白・閉じる前の `\`・`"`）
    #[test]
    fn windows_arguments_are_quoted() {
        assert_eq!(
            quote_windows(r"C:\Program Files\a.md"),
            r#""C:\Program Files\a.md""#
        );
        assert_eq!(quote_windows(r"C:\dir\"), r#""C:\dir\\""#);
        assert_eq!(quote_windows(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_windows("100%予定.md"), "\"100%予定.md\"");
    }

    /// 上げた側の書き方: 作り直さず中身を書く
    #[test]
    fn the_helper_writes_the_contents() {
        let dir = std::env::temp_dir().join(format!("mdview-elevate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("from.md"), dir.join("to.md"));
        std::fs::write(&from, "新しい中身").unwrap();
        std::fs::write(&to, "古い中身・長い").unwrap();
        assert_eq!(write_as_helper(&from, &to), 0);
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "新しい中身");
        assert_eq!(write_as_helper(&dir.join("無い.md"), &to), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
