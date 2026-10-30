//! タスクバー（Windows）・Dock（macOS）の右クリックの「最近使ったもの」（v3 R-18）。
//!
//! 開いた・保存したファイルを OS に知らせる。並べるのは OS の仕事（アプリの「最近使ったファイル」とは別に OS が覚える）。
//!
//! - Windows: `SHAddToRecentDocs`。インストーラーが `.md`・`.markdown` をこのアプリの扱える型として登録しているので、
//!   ジャンプリストの「最近使ったもの」に出る
//! - macOS: `NSDocumentController` の `noteNewRecentDocumentURL:`。`Info.plist` の `CFBundleDocumentTypes` があるので、
//!   Dock の「最近使った項目」に出る
//! - Linux など: 知らせる決まった仕組みが無い（`.desktop` の「新しいウィンドウ」の動作だけを置く。できる範囲）
//!
//! アプリの「一覧を消す」では、**OS の一覧も消す**（Windows は `IApplicationDestinations::RemoveAllDestinations`、
//! macOS は `clearRecentDocuments:`）。消えるのはこのアプリの一覧だけ（エクスプローラーの「最近使ったもの」は残る）

use std::path::Path;

/// 開いた・保存したファイルを OS に知らせる。**失敗しても何もしない**（並ばないだけ）
pub fn note(path: &Path) {
    imp::note(path);
}

/// OS の一覧（ジャンプリスト・Dock の最近使った項目）を消す（アプリの「一覧を消す」と一緒に）。**失敗しても何もしない**
pub fn clear() {
    imp::clear();
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::UI::Shell::{SHAddToRecentDocs, SHARD_PATHW};

    pub fn note(path: &Path) {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: 0 で終わる UTF-16 のパスを渡すだけ（OS が写す）
        unsafe { SHAddToRecentDocs(SHARD_PATHW as u32, wide.as_ptr().cast()) };
    }

    /// `IApplicationDestinations` の呼び口（IUnknown の 3 つに続く 3 つ）。**windows-sys は COM の型を持たない**ので並びを書く
    #[repr(C)]
    struct DestinationsVtbl {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(*mut Destinations) -> u32,
        set_app_id: usize,
        remove_destination: usize,
        remove_all_destinations: unsafe extern "system" fn(*mut Destinations) -> i32,
    }

    #[repr(C)]
    struct Destinations {
        vtbl: *const DestinationsVtbl,
    }

    /// CLSID_ApplicationDestinations
    const CLSID: windows_sys::core::GUID =
        windows_sys::core::GUID::from_u128(0x86c14003_4d6b_4ef3_a7b4_0506663b2e68);
    /// IID_IApplicationDestinations
    const IID: windows_sys::core::GUID =
        windows_sys::core::GUID::from_u128(0x12337d35_94c6_48a0_bce7_6a9c69d4d600);

    /// ジャンプリストの「最近使ったもの」を消す（`RemoveAllDestinations`）
    pub fn clear() {
        use windows_sys::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        // SAFETY: COM の決まった手順（初期化・作る・呼ぶ・放す・後始末）。窓の糸はすでに初期化されていることがある
        unsafe {
            let initialized = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let mut object: *mut Destinations = std::ptr::null_mut();
            let created = CoCreateInstance(
                &CLSID,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID,
                (&mut object as *mut *mut Destinations).cast(),
            );
            if created >= 0 && !object.is_null() {
                ((*(*object).vtbl).remove_all_destinations)(object);
                ((*(*object).vtbl).release)(object);
            }
            // 自分で初期化したとき（S_OK・S_FALSE）だけ後始末する
            if initialized >= 0 {
                CoUninitialize();
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::Path;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::{NSString, NSURL};

    pub fn note(path: &Path) {
        let Some(text) = path.to_str() else {
            return;
        };
        let text = NSString::from_str(text);
        let url = NSURL::fileURLWithPath(&text);
        // SAFETY: AppKit の決まった呼び方（窓の糸から呼ぶ。iced の update は窓の糸で動く）
        unsafe {
            let controller: Option<Retained<AnyObject>> =
                msg_send![class!(NSDocumentController), sharedDocumentController];
            if let Some(controller) = controller {
                let _: () = msg_send![&*controller, noteNewRecentDocumentURL: &*url];
            }
        }
    }

    /// Dock の「最近使った項目」を消す（`clearRecentDocuments:`）
    pub fn clear() {
        // SAFETY: AppKit の決まった呼び方（窓の糸から呼ぶ）
        unsafe {
            let controller: Option<Retained<AnyObject>> =
                msg_send![class!(NSDocumentController), sharedDocumentController];
            if let Some(controller) = controller {
                let nothing: Option<&AnyObject> = None;
                let _: () = msg_send![&*controller, clearRecentDocuments: nothing];
            }
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    pub fn note(_path: &std::path::Path) {}

    pub fn clear() {}
}

#[cfg(test)]
mod tests {
    /// OS の一覧を消す呼び出しが落ちない（Windows は COM を作って呼ぶ。**試験の実行ファイル自身の一覧**なので利用者の一覧は触らない）
    #[test]
    fn clearing_the_os_list_does_not_fail() {
        super::clear();
        super::clear();
    }
}
