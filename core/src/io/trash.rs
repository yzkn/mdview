//! ファイルを OS のごみ箱へ入れる（v2.7.0 R-03。フォルダーの比較からまとめて写すときに消すもの）。
//!
//! - **Windows**: シェルの消す操作（取り消せる形。エクスプローラーのごみ箱と同じ）
//! - **macOS**: 自分のごみ箱（`~/.Trash`）へ動かす。「元に戻す」の記録は残らない
//! - **Linux**: freedesktop.org の決まり（`$XDG_DATA_HOME/Trash`）。`files` へ動かし、`info` に元の場所を書く
//!
//! **ごみ箱へ入れられなければ消さない**（呼ぶ側が知らせる）。別の装置（ドライブ）にあって動かせないときも同じ。

use std::path::Path;

/// ごみ箱へ入れる。入れられなければ理由
pub fn to_trash(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{}: ファイルがありません", path.display()));
    }
    platform::to_trash(path)
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    pub fn to_trash(path: &Path) -> Result<(), String> {
        // 絶対パスで、終わりに 0 を 2 つ（並べて渡す形）
        let full = std::path::absolute(path).map_err(|error| error.to_string())?;
        let mut from: Vec<u16> = full.as_os_str().encode_wide().collect();
        from.extend([0, 0]);
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };
        // SAFETY: `from` は呼び出しの間生きていて、0 を 2 つで終わる
        let result = unsafe { SHFileOperationW(&mut operation) };
        if result != 0 || operation.fAnyOperationsAborted != 0 {
            return Err(format!(
                "{}: ごみ箱へ入れられませんでした（{result:#x}）",
                path.display()
            ));
        }
        if path.exists() {
            // ごみ箱の無いドライブでは、確かめずに消すことがある。消えていなければ失敗とみなす
            return Err(format!("{}: ごみ箱へ入れられませんでした", path.display()));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    pub fn to_trash(path: &Path) -> Result<(), String> {
        let home = std::env::var_os("HOME").ok_or("HOME がありません")?;
        let trash = Path::new(&home).join(".Trash");
        super::move_into(path, &trash).map(|_| ()).map_err(|error| {
            format!(
                "{}: ごみ箱へ入れられませんでした（{error}）",
                path.display()
            )
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::path::{Path, PathBuf};

    fn trash_dir() -> Option<PathBuf> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share"))
            })?;
        Some(data.join("Trash"))
    }

    pub fn to_trash(path: &Path) -> Result<(), String> {
        let fail = |error: String| {
            format!(
                "{}: ごみ箱へ入れられませんでした（{error}）",
                path.display()
            )
        };
        let trash = trash_dir().ok_or_else(|| fail("HOME がありません".to_owned()))?;
        let full = std::path::absolute(path).map_err(|error| fail(error.to_string()))?;
        std::fs::create_dir_all(trash.join("info")).map_err(|error| fail(error.to_string()))?;
        let name = super::move_into(path, &trash.join("files"))
            .map_err(|error| fail(error.to_string()))?;
        let info = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            encode(&full.to_string_lossy()),
            now()
        );
        // 記録が書けなくても、ファイルはごみ箱にある
        let _ = std::fs::write(trash.join("info").join(format!("{name}.trashinfo")), info);
        Ok(())
    }

    /// URL の形で書く（`/` と英数字のほかは `%xx`）
    fn encode(path: &str) -> String {
        let mut out = String::new();
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
                out.push(byte as char);
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    /// いまの時刻（`YYYY-MM-DDThh:mm:ss`。協定世界時で書く）
    fn now() -> String {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let (days, rest) = (seconds / 86_400, seconds % 86_400);
        // 1970-01-01 からの日数を暦に直す（Howard Hinnant の方法）
        let z = days as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
            rest / 3600,
            rest / 60 % 60,
            rest % 60
        )
    }
}

/// フォルダーへ動かす（同じ名前があれば番号を付ける）。付けた名前を返す。**別の装置へは動かさない**
#[cfg(unix)]
fn move_into(path: &Path, folder: &Path) -> std::io::Result<String> {
    std::fs::create_dir_all(folder)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_owned(), format!(".{extension}")),
        _ => (name.clone(), String::new()),
    };
    let mut candidate = name.clone();
    let mut number = 1;
    while folder.join(&candidate).exists() {
        number += 1;
        candidate = format!("{stem} {number}{extension}");
    }
    // `rename` は別の装置へは動かせない（そのときは失敗にして消さない）
    std::fs::rename(path, folder.join(&candidate))?;
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_are_not_trashed() {
        let path = std::env::temp_dir().join("mdview-trash-missing.md");
        let _ = std::fs::remove_file(&path);
        assert!(to_trash(&path).is_err());
    }
}
