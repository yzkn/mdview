//! OS に中継先（プロキシ）を聞く（v2.6.0 R-02）。
//!
//! **Windows は WinHTTP に任せる。** 自動構成スクリプト（PAC）・自動検出（WPAD）・手動の設定のどれでも、
//! OS（ブラウザーの設定）と同じ中継先になる。PAC は JavaScript なので、自前で読むには処理系が要るが、
//! WinHTTP なら OS が評価する。
//!
//! **macOS も OS に任せる**（v2.7.0 R-04）。CFNetwork にシステムの設定を聞き、PAC なら CFNetwork に評価させる。
//! **Linux は GNOME の手動の設定だけ**（v2.7.0 R-04）。`gsettings` で `org.gnome.system.proxy` を読み、手動なら
//! その中継先を使う。自動（PAC）・読めないときは直接つなぐ（JavaScript の処理系は入れない）。
//!
//! 並びの読み分け（[`pick`]）・除外の当て方（[`bypassed`]）・GNOME の値の読み方は OS に依らない関数にして試験する。

/// `url` へつなぐときの中継先（`http://host:port`）。直接つなぐ・聞けないなら `None`
pub fn proxy_for(url: &str) -> Option<String> {
    imp::proxy_for(url)
}

/// WinHTTP が返す中継先の並び（`host:port;host2:port`・`http=host:port;https=host2:port`）から、
/// URL の種類に合うものを 1 つ選んで `http://host:port` にする
pub fn pick(list: &str, https: bool) -> Option<String> {
    let wanted = if https { "https=" } else { "http=" };
    let items: Vec<&str> = list
        .split([';', ' ', '\t'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect();
    let chosen = items
        .iter()
        .find_map(|item| item.strip_prefix(wanted))
        .or_else(|| items.iter().copied().find(|item| !item.contains('=')))?;
    let chosen = chosen.trim_start_matches("http://").trim_end_matches('/');
    (!chosen.is_empty()).then(|| format!("http://{chosen}"))
}

/// 除外の並び（`<local>`・`*.example.com`・`10.*` を `;` で区切ったもの）に、つなぐ先が当たるか
pub fn bypassed(host: &str, list: &str) -> bool {
    let host = host.to_ascii_lowercase();
    list.split([';', ' ', '\t'])
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .any(|item| {
            if item == "<local>" {
                !host.contains('.')
            } else {
                glob(&item, &host)
            }
        })
}

/// `*` だけを使う当て方
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (index, part) in parts.iter().enumerate() {
        if index == 0 {
            let Some(after) = rest.strip_prefix(part) else {
                return false;
            };
            rest = after;
        } else if index == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(found) = rest.find(part) else {
                return false;
            };
            rest = &rest[found + part.len()..];
        }
    }
    true
}

/// URL の相手の名前（利用者名・ポートを除く）
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or(inner)
    } else {
        authority.split(':').next().unwrap_or(authority)
    };
    host.to_ascii_lowercase()
}

#[cfg(windows)]
mod imp {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpGetIEProxyConfigForCurrentUser, WinHttpGetProxyForUrl,
        WinHttpOpen, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTOPROXY_AUTO_DETECT,
        WINHTTP_AUTOPROXY_CONFIG_URL, WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_AUTO_DETECT_TYPE_DHCP,
        WINHTTP_AUTO_DETECT_TYPE_DNS_A, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// WinHTTP が渡した文字を読んで放す
    ///
    /// # Safety
    /// `pointer` は WinHTTP が `GlobalAlloc` で取った、0 で終わる UTF-16 か null
    unsafe fn take(pointer: *mut u16) -> Option<String> {
        if pointer.is_null() {
            return None;
        }
        let mut length = 0usize;
        // SAFETY: 0 で終わる文字列（WinHTTP の約束）
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: いま数えた長さの中だけを読む
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
        // SAFETY: WinHTTP の文字は GlobalFree で放す決まり
        unsafe { GlobalFree(pointer.cast()) };
        Some(text)
    }

    pub fn proxy_for(url: &str) -> Option<String> {
        let https = url.trim().to_ascii_lowercase().starts_with("https://");
        // SAFETY: 構造体は 0 で埋めて渡す決まり。中の文字は `take` が読んで放す
        let mut config: WINHTTP_CURRENT_USER_IE_PROXY_CONFIG = unsafe { std::mem::zeroed() };
        if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } == 0 {
            return None;
        }
        let script = unsafe { take(config.lpszAutoConfigUrl) };
        let manual = unsafe { take(config.lpszProxy) };
        let bypass = unsafe { take(config.lpszProxyBypass) };
        let detect = config.fAutoDetect != 0;
        // 自動（PAC・自動検出）を先に聞く。答えが出なければ手動の設定へ
        if detect || script.is_some() {
            if let Some(answer) = automatic(url, https, detect, script.as_deref()) {
                return answer;
            }
        }
        let manual = manual?;
        if bypass
            .as_deref()
            .is_some_and(|list| super::bypassed(&super::host_of(url), list))
        {
            return None;
        }
        super::pick(&manual, https)
    }

    /// PAC・自動検出で聞く。`Some(None)` は「直接つなぐ」、`None` は「答えが出なかった」
    fn automatic(
        url: &str,
        https: bool,
        detect: bool,
        script: Option<&str>,
    ) -> Option<Option<String>> {
        let agent = wide(concat!("mdview/", env!("CARGO_PKG_VERSION")));
        // SAFETY: 名前は 0 で終わる UTF-16。中継の名前と除外は使わない（null）
        let session = unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                null(),
                null(),
                0,
            )
        };
        if session.is_null() {
            return None;
        }
        let script = script.map(wide);
        let target = wide(url);
        // SAFETY: 0 で埋めてから要る所だけを入れる
        let mut options: WINHTTP_AUTOPROXY_OPTIONS = unsafe { std::mem::zeroed() };
        options.dwFlags = if detect {
            WINHTTP_AUTOPROXY_AUTO_DETECT
        } else {
            0
        } | if script.is_some() {
            WINHTTP_AUTOPROXY_CONFIG_URL
        } else {
            0
        };
        options.dwAutoDetectFlags = if detect {
            WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A
        } else {
            0
        };
        options.lpszAutoConfigUrl = script.as_ref().map_or(null(), |text| text.as_ptr());
        options.lpvReserved = null_mut();
        options.fAutoLogonIfChallenged = 1;
        let mut info: WINHTTP_PROXY_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: どの文字も、この呼び出しの間は生きている
        let found =
            unsafe { WinHttpGetProxyForUrl(session, target.as_ptr(), &mut options, &mut info) }
                != 0;
        // SAFETY: 開いたものを閉じる
        unsafe { WinHttpCloseHandle(session) };
        if !found {
            return None;
        }
        let list = unsafe { take(info.lpszProxy) };
        let _ = unsafe { take(info.lpszProxyBypass) };
        if info.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY {
            return Some(None);
        }
        Some(list.and_then(|list| super::pick(&list, https)))
    }
}

/// GNOME の設定の文字の値（`'manual'`）から引用符を外す
pub fn gvariant_string(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            value
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })?;
    Some(inner.replace("\\'", "'"))
}

/// GNOME の設定の文字の並び（`['localhost', '*.local']`・`@as []`）を読む
pub fn gvariant_strings(value: &str) -> Vec<String> {
    let value = value.trim().trim_start_matches("@as").trim();
    let Some(inner) = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Vec::new();
    };
    inner
        .split(',')
        .filter_map(gvariant_string)
        .filter(|item| !item.is_empty())
        .collect()
}

/// GNOME の手動の設定から中継先を決める（`mode`・種類ごとの名前とポート・除外の並び・つなぐ先）
pub fn gnome_proxy(
    mode: &str,
    host: Option<&str>,
    port: Option<u16>,
    ignore: &[String],
    url_host: &str,
) -> Option<String> {
    if mode != "manual" {
        return None;
    }
    let host = host.map(str::trim).filter(|host| !host.is_empty())?;
    if ignore.iter().any(|item| ignored_by_gnome(url_host, item)) {
        return None;
    }
    let host = host.trim_start_matches("http://").trim_end_matches('/');
    Some(match port.filter(|port| *port != 0) {
        Some(port) => format!("http://{host}:{port}"),
        None => format!("http://{host}"),
    })
}

/// デスクトップの設定が言う中継の仕方（v2.8.0 R-03。Linux）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopProxy {
    /// 直接つなぐ（中継しない・除外に当たった・読めない）
    Direct,
    /// 手動の中継先（`http://host:port`）
    Manual(String),
    /// 自動構成スクリプト（PAC）の URL
    Script(String),
}

/// KDE の設定（`kioslaverc` の `[Proxy Settings]`）を読む（v2.8.0 R-03）。`None` は設定が無い
///
/// `ProxyType`: 0 = 使わない・1 = 手動・2 = PAC・3 = 自動検出・4 = 環境変数。自動検出は扱わず直接つなぐ。
/// 環境変数はこれより先に見ているので、4 も直接つなぐ
pub fn kde_proxy(config: &str, https: bool, url_host: &str) -> Option<DesktopProxy> {
    let mut in_section = false;
    let mut values = std::collections::HashMap::new();
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[Proxy Settings]";
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // `httpProxy[$e]` のような印は外す
            let key = key.split('[').next().unwrap_or(key).trim();
            values.insert(key.to_owned(), value.trim().to_owned());
        }
    }
    let kind = values.get("ProxyType")?.parse::<u32>().ok()?;
    Some(match kind {
        1 => {
            let reversed = values
                .get("ReversedException")
                .is_some_and(|value| value == "true");
            let listed = values.get("NoProxyFor").is_some_and(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .any(|item| {
                        ignored_by_gnome(url_host, item.trim_start_matches('.')) || {
                            // `.example.com` は下の名前も含む
                            item.starts_with('.') && url_host.ends_with(item)
                        }
                    })
            });
            // 逆にしていれば、並べたものだけを中継する
            if listed != reversed {
                return Some(DesktopProxy::Direct);
            }
            let key = if https { "httpsProxy" } else { "httpProxy" };
            match values.get(key).and_then(|value| kde_address(value)) {
                Some(address) => DesktopProxy::Manual(address),
                None => DesktopProxy::Direct,
            }
        }
        2 => values
            .get("Proxy Config Script")
            .filter(|url| !url.is_empty())
            .map_or(DesktopProxy::Direct, |url| {
                DesktopProxy::Script(url.clone())
            }),
        _ => DesktopProxy::Direct,
    })
}

/// KDE の中継先の書き方（`http://host:port`・`host:port`・`host port`）を `http://host:port` にする
fn kde_address(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or(value)
        .trim_end_matches('/');
    let rest = match rest.rsplit_once(' ') {
        Some((host, port)) if port.parse::<u16>().is_ok() => format!("{}:{port}", host.trim()),
        _ => rest.to_owned(),
    };
    (!rest.is_empty()).then(|| format!("http://{rest}"))
}

/// GNOME の除外の 1 つ（名前・`*.example.com`・`10.0.0.0/8` の形）に当たるか
fn ignored_by_gnome(host: &str, item: &str) -> bool {
    let item = item.trim().to_ascii_lowercase();
    if let Some((network, bits)) = item.split_once('/') {
        let (Ok(network), Ok(bits), Ok(address)) = (
            network.parse::<std::net::Ipv4Addr>(),
            bits.parse::<u32>(),
            host.parse::<std::net::Ipv4Addr>(),
        ) else {
            return false;
        };
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits.min(32))
        };
        return u32::from(network) & mask == u32::from(address) & mask;
    }
    glob(&item, &host.to_ascii_lowercase())
}

#[cfg(target_os = "macos")]
mod imp {
    //! CFNetwork に聞く。PAC は CFNetwork に評価させ、答えが出るまで（最長 `WAIT` 秒）この糸の中で待つ
    use std::ffi::{c_void, CStr};
    use std::os::raw::{c_char, c_long};
    use std::ptr::null;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFURLRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFRunLoopRef = *const c_void;
    type CFRunLoopSourceRef = *const c_void;
    type CFErrorRef = *const c_void;
    type CFIndex = c_long;

    #[repr(C)]
    struct CFStreamClientContext {
        version: CFIndex,
        info: *mut c_void,
        retain: *const c_void,
        release: *const c_void,
        copy_description: *const c_void,
    }

    /// kCFStringEncodingUTF8
    const UTF8: u32 = 0x0800_0100;
    /// kCFNumberSInt32Type
    const NUMBER_SINT32: CFIndex = 3;
    /// PAC の答えを待つ長さ（秒）
    const WAIT: f64 = 10.0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: CFTypeRef);
        fn CFRetain(value: CFTypeRef) -> CFTypeRef;
        fn CFStringCreateWithBytes(
            allocator: CFTypeRef,
            bytes: *const u8,
            length: CFIndex,
            encoding: u32,
            external: u8,
        ) -> CFStringRef;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut c_char,
            size: CFIndex,
            encoding: u32,
        ) -> u8;
        fn CFStringCompare(a: CFStringRef, b: CFStringRef, options: usize) -> CFIndex;
        fn CFURLCreateWithString(
            allocator: CFTypeRef,
            text: CFStringRef,
            base: CFURLRef,
        ) -> CFURLRef;
        fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
        fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        fn CFNumberGetValue(number: CFTypeRef, kind: CFIndex, out: *mut c_void) -> u8;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(
            run_loop: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRemoveSource(
            run_loop: CFRunLoopRef,
            source: CFRunLoopSourceRef,
            mode: CFStringRef,
        );
        fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, once: u8) -> i32;
        fn CFRunLoopStop(run_loop: CFRunLoopRef);
    }

    #[link(name = "CFNetwork", kind = "framework")]
    extern "C" {
        static kCFProxyTypeKey: CFStringRef;
        static kCFProxyHostNameKey: CFStringRef;
        static kCFProxyPortNumberKey: CFStringRef;
        static kCFProxyAutoConfigurationURLKey: CFStringRef;
        static kCFProxyTypeNone: CFStringRef;
        static kCFProxyTypeHTTP: CFStringRef;
        static kCFProxyTypeHTTPS: CFStringRef;
        static kCFProxyTypeAutoConfigurationURL: CFStringRef;
        fn CFNetworkCopySystemProxySettings() -> CFDictionaryRef;
        fn CFNetworkCopyProxiesForURL(url: CFURLRef, settings: CFDictionaryRef) -> CFArrayRef;
        fn CFNetworkExecuteProxyAutoConfigurationURL(
            script: CFURLRef,
            target: CFURLRef,
            callback: extern "C" fn(*mut c_void, CFArrayRef, CFErrorRef),
            context: *mut CFStreamClientContext,
        ) -> CFRunLoopSourceRef;
    }

    /// 作った・写した CF のもの。抜けるときに放す
    struct Owned(CFTypeRef);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: Create・Copy・Retain で受け取ったもの（放す決まり）
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn cf_string(text: &str) -> Owned {
        // SAFETY: 長さを渡すので 0 で終わらなくてよい
        Owned(unsafe {
            CFStringCreateWithBytes(null(), text.as_ptr(), text.len() as CFIndex, UTF8, 0)
        })
    }

    fn rust_string(value: CFTypeRef) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let mut buffer = vec![0 as c_char; 1024];
        // SAFETY: 入れ物の大きさを渡す
        let ok = unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), 1024, UTF8) };
        if ok == 0 {
            return None;
        }
        // SAFETY: CFStringGetCString は 0 で終える
        Some(
            unsafe { CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn same(a: CFStringRef, b: CFStringRef) -> bool {
        // SAFETY: どちらも CFString
        !a.is_null() && !b.is_null() && unsafe { CFStringCompare(a, b, 0) } == 0
    }

    /// 並びの頭から順に見て、最初に使えるもの。`Some(None)` は「直接つなぐ」、`None` は「答えが無い」
    fn choose(list: CFArrayRef, target: CFURLRef) -> Option<Option<String>> {
        if list.is_null() {
            return None;
        }
        // SAFETY: 並びの中は CFDictionary、鍵は CFNetwork の定数（CFNetwork の約束）
        unsafe {
            for index in 0..CFArrayGetCount(list) {
                let proxy = CFArrayGetValueAtIndex(list, index);
                let kind = CFDictionaryGetValue(proxy, kCFProxyTypeKey);
                if same(kind, kCFProxyTypeNone) {
                    return Some(None);
                }
                if same(kind, kCFProxyTypeHTTP) || same(kind, kCFProxyTypeHTTPS) {
                    let Some(host) = rust_string(CFDictionaryGetValue(proxy, kCFProxyHostNameKey))
                    else {
                        continue;
                    };
                    let number = CFDictionaryGetValue(proxy, kCFProxyPortNumberKey);
                    let mut port: i32 = 0;
                    if !number.is_null() {
                        CFNumberGetValue(number, NUMBER_SINT32, (&mut port as *mut i32).cast());
                    }
                    return Some(Some(if port > 0 {
                        format!("http://{host}:{port}")
                    } else {
                        format!("http://{host}")
                    }));
                }
                if same(kind, kCFProxyTypeAutoConfigurationURL) {
                    let script = CFDictionaryGetValue(proxy, kCFProxyAutoConfigurationURLKey);
                    if let Some(answer) = run_script(script, target) {
                        return Some(answer);
                    }
                }
            }
        }
        None
    }

    /// PAC の答えを受け取る所
    struct Answer {
        list: CFArrayRef,
        done: bool,
    }

    extern "C" fn received(info: *mut c_void, list: CFArrayRef, _error: CFErrorRef) {
        // SAFETY: `info` は `run_script` の `Answer`（待っている間は生きている）
        let answer = unsafe { &mut *info.cast::<Answer>() };
        if !list.is_null() {
            // SAFETY: 呼び出しの後も使うので持っておく（`run_script` が放す）
            answer.list = unsafe { CFRetain(list) };
        }
        answer.done = true;
        // SAFETY: いまの糸の回し（`run_script` が回している）を止める
        unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) };
    }

    /// PAC を CFNetwork に評価させ、答えの並びから選ぶ
    fn run_script(script: CFURLRef, target: CFURLRef) -> Option<Option<String>> {
        if script.is_null() {
            return None;
        }
        let mut answer = Answer {
            list: null(),
            done: false,
        };
        let mut context = CFStreamClientContext {
            version: 0,
            info: (&mut answer as *mut Answer).cast(),
            retain: null(),
            release: null(),
            copy_description: null(),
        };
        // SAFETY: 渡すものは呼び出しの間生きている
        let source = unsafe {
            CFNetworkExecuteProxyAutoConfigurationURL(script, target, received, &mut context)
        };
        if source.is_null() {
            return None;
        }
        let source = Owned(source);
        let mode = cf_string("mdview.proxy");
        // SAFETY: 自分だけの回し方（mode）で回すので、ほかの仕事は動かない
        unsafe {
            let run_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(run_loop, source.0, mode.0);
            let started = std::time::Instant::now();
            while !answer.done && started.elapsed().as_secs_f64() < WAIT {
                CFRunLoopRunInMode(mode.0, WAIT, 0);
            }
            CFRunLoopRemoveSource(run_loop, source.0, mode.0);
        }
        let list = Owned(answer.list);
        choose(list.0, target)
    }

    pub fn proxy_for(url: &str) -> Option<String> {
        let text = cf_string(url);
        // SAFETY: CF の作る・写す関数の約束に従い、`Owned` で放す
        let target = Owned(unsafe { CFURLCreateWithString(null(), text.0, null()) });
        if target.0.is_null() {
            return None;
        }
        let settings = Owned(unsafe { CFNetworkCopySystemProxySettings() });
        if settings.0.is_null() {
            return None;
        }
        let list = Owned(unsafe { CFNetworkCopyProxiesForURL(target.0, settings.0) });
        choose(list.0, target.0).flatten()
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    //! GNOME の設定（`gsettings`）と KDE の設定（`kioslaverc`。v2.8.0 R-03）を読む。
    //! KDE の机の上では KDE を先に、ほかでは GNOME を先に見て、読めなければもう一方を見る
    use std::path::PathBuf;
    use std::process::Command;

    use super::DesktopProxy;

    fn get(schema: &str, key: &str) -> Option<String> {
        let output = Command::new("gsettings")
            .args(["get", schema, key])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// GNOME の設定。`gsettings` が無ければ `None`
    fn gnome(url: &str, https: bool, host: &str) -> Option<DesktopProxy> {
        let mode = super::gvariant_string(&get("org.gnome.system.proxy", "mode")?)?;
        Some(match mode.as_str() {
            "manual" => {
                let schema = if https {
                    "org.gnome.system.proxy.https"
                } else {
                    "org.gnome.system.proxy.http"
                };
                let name = get(schema, "host").and_then(|value| super::gvariant_string(&value));
                let port = get(schema, "port").and_then(|value| value.trim().parse::<u16>().ok());
                let ignore = get("org.gnome.system.proxy", "ignore-hosts")
                    .map(|value| super::gvariant_strings(&value))
                    .unwrap_or_default();
                match super::gnome_proxy(&mode, name.as_deref(), port, &ignore, host) {
                    Some(address) => DesktopProxy::Manual(address),
                    None => DesktopProxy::Direct,
                }
            }
            "auto" => get("org.gnome.system.proxy", "autoconfig-url")
                .and_then(|value| super::gvariant_string(&value))
                .filter(|script| !script.is_empty())
                .map_or(DesktopProxy::Direct, DesktopProxy::Script),
            _ => {
                let _ = url;
                DesktopProxy::Direct
            }
        })
    }

    /// KDE の設定。ファイルが無ければ `None`
    fn kde(https: bool, host: &str) -> Option<DesktopProxy> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        let text = std::fs::read_to_string(base.join("kioslaverc")).ok()?;
        super::kde_proxy(&text, https, host)
    }

    pub fn proxy_for(url: &str) -> Option<String> {
        let https = url.trim().to_ascii_lowercase().starts_with("https://");
        let host = super::host_of(url);
        let kde_first = std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|desktop| desktop.to_ascii_uppercase().contains("KDE"));
        let answer = if kde_first {
            kde(https, &host).or_else(|| gnome(url, https, &host))
        } else {
            gnome(url, https, &host).or_else(|| kde(https, &host))
        };
        match answer? {
            DesktopProxy::Direct => None,
            DesktopProxy::Manual(address) => Some(address),
            DesktopProxy::Script(script) => super::script_proxy(&script, url),
        }
    }
}

/// 自動構成スクリプト（PAC）で中継先を決める（Linux。v2.8.0 R-06。組み込んだ処理系で評価する）
#[cfg(all(unix, not(target_os = "macos")))]
fn script_proxy(script: &str, url: &str) -> Option<String> {
    super::pac::proxy_for(script, url, &host_of(url))
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn proxy_for(_url: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_read_by_scheme() {
        assert_eq!(
            pick("proxy:8080", true).as_deref(),
            Some("http://proxy:8080")
        );
        assert_eq!(
            pick("http=a:80;https=b:443", true).as_deref(),
            Some("http://b:443")
        );
        assert_eq!(
            pick("http=a:80;https=b:443", false).as_deref(),
            Some("http://a:80")
        );
        // 種類ごとの指定が無ければ、種類の無いもの
        assert_eq!(
            pick("ftp=c:21;d:3128", true).as_deref(),
            Some("http://d:3128")
        );
        assert_eq!(
            pick("first:1; second:2", false).as_deref(),
            Some("http://first:1")
        );
        assert_eq!(pick("", true), None);
    }

    #[test]
    fn bypass_lists_match_hosts() {
        assert!(bypassed("intranet", "<local>"));
        assert!(!bypassed("example.com", "<local>"));
        assert!(bypassed("docs.example.com", "*.example.com;10.*"));
        assert!(bypassed("10.1.2.3", "*.example.com;10.*"));
        assert!(!bypassed("example.org", "*.example.com;10.*"));
        assert!(bypassed("Host.Example.com", "host.example.com"));
    }

    /// GNOME の値の読み方と、手動のときだけ中継すること（v2.7.0 R-04）
    #[test]
    fn gnome_settings_are_read() {
        assert_eq!(gvariant_string("'manual'").as_deref(), Some("manual"));
        assert_eq!(gvariant_string("manual"), None);
        assert_eq!(
            gvariant_strings("['localhost', '127.0.0.0/8', '*.local']"),
            ["localhost", "127.0.0.0/8", "*.local"]
        );
        assert!(gvariant_strings("@as []").is_empty());
        let ignore = gvariant_strings("['localhost', '10.0.0.0/8', '*.corp']");
        assert_eq!(
            gnome_proxy("manual", Some("proxy"), Some(8080), &ignore, "example.com").as_deref(),
            Some("http://proxy:8080")
        );
        assert_eq!(
            gnome_proxy("auto", Some("proxy"), Some(8080), &ignore, "example.com"),
            None,
            "自動（PAC）は直接つなぐ"
        );
        assert_eq!(
            gnome_proxy("manual", Some(""), Some(8080), &ignore, "example.com"),
            None
        );
        assert_eq!(
            gnome_proxy("manual", Some("proxy"), Some(80), &ignore, "10.1.2.3"),
            None
        );
        assert_eq!(
            gnome_proxy("manual", Some("proxy"), Some(80), &ignore, "a.corp"),
            None
        );
        assert_eq!(
            gnome_proxy("manual", Some("proxy"), Some(80), &ignore, "11.1.2.3").as_deref(),
            Some("http://proxy:80")
        );
        assert_eq!(
            gnome_proxy("manual", Some("proxy"), Some(0), &[], "x").as_deref(),
            Some("http://proxy")
        );
    }

    /// KDE の設定の読み方（v2.8.0 R-03）
    #[test]
    fn kde_settings_are_read() {
        let config = "[General]\nx=1\n\n[Proxy Settings]\nProxyType=1\nhttpProxy=http://proxy 8080\nhttpsProxy=secure:3128\nNoProxyFor=localhost,.corp,10.0.0.0/8\n";
        assert_eq!(
            kde_proxy(config, true, "example.com"),
            Some(DesktopProxy::Manual("http://secure:3128".to_owned()))
        );
        assert_eq!(
            kde_proxy(config, false, "example.com"),
            Some(DesktopProxy::Manual("http://proxy:8080".to_owned()))
        );
        assert_eq!(
            kde_proxy(config, true, "a.corp"),
            Some(DesktopProxy::Direct)
        );
        assert_eq!(
            kde_proxy(config, true, "10.2.3.4"),
            Some(DesktopProxy::Direct)
        );
        assert_eq!(
            kde_proxy(config, true, "localhost"),
            Some(DesktopProxy::Direct)
        );
        let reversed = config.replace("ProxyType=1", "ProxyType=1\nReversedException=true");
        assert_eq!(
            kde_proxy(&reversed, true, "example.com"),
            Some(DesktopProxy::Direct)
        );
        assert!(matches!(
            kde_proxy(&reversed, true, "a.corp"),
            Some(DesktopProxy::Manual(_))
        ));
        let script = "[Proxy Settings]\nProxyType=2\nProxy Config Script=http://wpad/proxy.pac\n";
        assert_eq!(
            kde_proxy(script, true, "x"),
            Some(DesktopProxy::Script("http://wpad/proxy.pac".to_owned()))
        );
        assert_eq!(
            kde_proxy("[Proxy Settings]\nProxyType=0\n", true, "x"),
            Some(DesktopProxy::Direct)
        );
        assert_eq!(
            kde_proxy("[Other]\nProxyType=1\n", true, "x"),
            None,
            "節が無い"
        );
    }

    #[test]
    fn hosts_are_taken_from_urls() {
        assert_eq!(host_of("https://user@Example.com:8443/a?b"), "example.com");
        assert_eq!(host_of("http://[::1]:80/"), "::1");
        assert_eq!(
            host_of("https://raw.githubusercontent.com"),
            "raw.githubusercontent.com"
        );
    }
}
