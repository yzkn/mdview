//! 自動構成スクリプト（PAC）を評価する（v2.8.0 R-06。**Linux だけ**。Windows・macOS は OS に任せる）。
//!
//! PAC は JavaScript なので、組み込んだ処理系（boa。純 Rust）で `FindProxyForURL(url, host)` を呼ぶ。
//! PAC の決まりの関数のうち、名前を引く 2 つ（`dnsResolve`・`myIpAddress`）は Rust で用意し、残りは決まりの JavaScript を先に読ませる。
//!
//! - PAC は URL ごとに取り直さない（`CACHE_FOR` の間は覚えておく）。取るときは**中継を通さない**（中継先を決めるための取得なので）
//! - 評価は別の糸で行い、`WAIT` を過ぎたら直接つなぐ（PAC が終わらなくても URL を開く仕事を止めない）
//! - 取れない・評価に失敗したときも直接つなぐ

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// PAC を覚えておく長さ
const CACHE_FOR: Duration = Duration::from_secs(300);
/// 取る・評価するのを待つ長さ
const WAIT: Duration = Duration::from_secs(10);
/// PAC の大きさの上限
const LIMIT: u64 = 1024 * 1024;

/// PAC の決まりの関数（名前を引く 2 つは Rust で用意する）
const PRELUDE: &str = r#"
function isPlainHostName(host) { return host.indexOf('.') < 0; }
function dnsDomainIs(host, domain) {
    return host.length >= domain.length && host.substring(host.length - domain.length) == domain;
}
function localHostOrDomainIs(host, hostdom) {
    return host == hostdom || hostdom.lastIndexOf(host + '.', 0) == 0;
}
function isResolvable(host) { return dnsResolve(host) != null; }
function convert_addr(ipchars) {
    var bytes = ipchars.split('.');
    return ((bytes[0] & 0xff) << 24) | ((bytes[1] & 0xff) << 16) | ((bytes[2] & 0xff) << 8) | (bytes[3] & 0xff);
}
function isInNet(ipaddr, pattern, maskstr) {
    if (!/^\d+\.\d+\.\d+\.\d+$/.test(ipaddr)) {
        ipaddr = dnsResolve(ipaddr);
        if (ipaddr == null) { return false; }
    }
    var host = convert_addr(ipaddr), pat = convert_addr(pattern), mask = convert_addr(maskstr);
    return (host & mask) == (pat & mask);
}
function dnsDomainLevels(host) { return host.split('.').length - 1; }
function shExpMatch(url, pattern) {
    pattern = pattern.replace(/\./g, '\\.').replace(/\*/g, '.*').replace(/\?/g, '.');
    return new RegExp('^' + pattern + '$').test(url);
}
var wdays = { SUN: 0, MON: 1, TUE: 2, WED: 3, THU: 4, FRI: 5, SAT: 6 };
function weekdayRange(wd1, wd2, gmt) {
    var today = (gmt == 'GMT') ? new Date().getUTCDay() : new Date().getDay();
    var a = wdays[wd1], b = (wd2 === undefined || wd2 == 'GMT') ? a : wdays[wd2];
    return a <= b ? (a <= today && today <= b) : (today >= a || today <= b);
}
function timeRange() {
    var args = Array.prototype.slice.call(arguments), gmt = args[args.length - 1] == 'GMT';
    if (gmt) { args.pop(); }
    var now = new Date(), hour = gmt ? now.getUTCHours() : now.getHours();
    if (args.length == 1) { return hour == args[0]; }
    return args[0] <= hour && hour < args[1];
}
function dateRange() { return true; }
"#;

/// `FindProxyForURL` の答え（`PROXY a:8080; DIRECT` など）から、最初に使える中継先。直接なら `None`
pub fn first_proxy(answer: &str) -> Option<String> {
    for item in answer.split(';') {
        let item = item.trim();
        let mut words = item.split_whitespace();
        match words.next().map(str::to_ascii_uppercase).as_deref() {
            Some("DIRECT") => return None,
            Some("PROXY") | Some("HTTP") | Some("HTTPS") => {
                if let Some(address) = words.next() {
                    let address = address.trim_start_matches("http://").trim_end_matches('/');
                    return Some(format!("http://{address}"));
                }
            }
            // SOCKS は使えないので次へ
            _ => {}
        }
    }
    None
}

/// PAC を評価して、`FindProxyForURL(url, host)` の答えを返す
pub fn evaluate(script: &str, url: &str, host: &str) -> Result<String, String> {
    use boa_engine::{js_string, Context, JsValue, NativeFunction, Source};

    let mut context = Context::default();
    let fail = |error: boa_engine::JsError| error.to_string();
    context
        .register_global_callable(
            js_string!("dnsResolve"),
            1,
            NativeFunction::from_fn_ptr(|_, args, context| {
                let host = args
                    .first()
                    .cloned()
                    .unwrap_or_default()
                    .to_string(context)?
                    .to_std_string_escaped();
                Ok(resolve(&host).map_or(JsValue::null(), |address| {
                    JsValue::from(js_string!(address))
                }))
            }),
        )
        .map_err(fail)?;
    context
        .register_global_callable(
            js_string!("myIpAddress"),
            0,
            NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::from(js_string!(my_address())))),
        )
        .map_err(fail)?;
    context.eval(Source::from_bytes(PRELUDE)).map_err(fail)?;
    context.eval(Source::from_bytes(script)).map_err(fail)?;
    // 引数は JSON の文字の書き方で渡す（そのまま JavaScript の文字になる）
    let call = format!(
        "FindProxyForURL({}, {})",
        serde_json::to_string(url).unwrap_or_default(),
        serde_json::to_string(host).unwrap_or_default()
    );
    let answer = context.eval(Source::from_bytes(&call)).map_err(fail)?;
    Ok(answer
        .to_string(&mut context)
        .map_err(fail)?
        .to_std_string_escaped())
}

/// 名前を IPv4 の番地に引く（引けなければ `None`）
fn resolve(host: &str) -> Option<String> {
    use std::net::ToSocketAddrs;
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return Some(host.to_owned());
    }
    (host, 0)
        .to_socket_addrs()
        .ok()?
        .find(|address| address.is_ipv4())
        .map(|address| address.ip().to_string())
}

/// 自分の番地（外へ出るときの番地。**送らずに**つなぐ先だけ決めて調べる）
fn my_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("192.0.2.1:9")?;
            socket.local_addr()
        })
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_owned())
}

/// 覚えている PAC（URL・取った時刻・中身）
static CACHE: Mutex<Option<(String, Instant, String)>> = Mutex::new(None);

/// PAC を取る（`file://` は手元のファイル。**中継を通さない**）。覚えていればそれを使う
fn script_text(script_url: &str) -> Result<String, String> {
    if let Ok(cache) = CACHE.lock() {
        if let Some((url, at, text)) = cache.as_ref() {
            if url == script_url && at.elapsed() < CACHE_FOR {
                return Ok(text.clone());
            }
        }
    }
    let text = if let Some(path) = script_url.strip_prefix("file://") {
        std::fs::read_to_string(path).map_err(|error| error.to_string())?
    } else {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(WAIT))
            .user_agent(concat!("mdview/", env!("CARGO_PKG_VERSION")))
            .proxy(None)
            .build()
            .into();
        agent
            .get(script_url)
            .call()
            .map_err(|error| error.to_string())?
            .body_mut()
            .with_config()
            .limit(LIMIT)
            .read_to_string()
            .map_err(|error| error.to_string())?
    };
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((script_url.to_owned(), Instant::now(), text.clone()));
    }
    Ok(text)
}

/// PAC で中継先を決める（直接・失敗・時間切れは `None`）
pub fn proxy_for(script_url: &str, url: &str, host: &str) -> Option<String> {
    let (script_url, url, host) = (script_url.to_owned(), url.to_owned(), host.to_owned());
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let answer = script_text(&script_url).and_then(|script| evaluate(&script, &url, &host));
        let _ = sender.send(answer);
    });
    let answer = receiver.recv_timeout(WAIT * 2).ok()?.ok()?;
    first_proxy(&answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read() {
        assert_eq!(
            first_proxy("PROXY proxy.example.com:8080; DIRECT").as_deref(),
            Some("http://proxy.example.com:8080")
        );
        assert_eq!(
            first_proxy("SOCKS s:1080; PROXY p:3128").as_deref(),
            Some("http://p:3128")
        );
        assert_eq!(first_proxy("DIRECT"), None);
        assert_eq!(first_proxy(""), None);
    }

    /// 決まりの関数を使う PAC を評価できる
    #[test]
    fn scripts_are_evaluated() {
        let script = r#"
            function FindProxyForURL(url, host) {
                if (isPlainHostName(host) || dnsDomainIs(host, ".corp.example")) { return "DIRECT"; }
                if (isInNet(host, "10.0.0.0", "255.0.0.0")) { return "DIRECT"; }
                if (shExpMatch(url, "https://*.github.com/*")) { return "PROXY gh-proxy:8080"; }
                if (dnsDomainLevels(host) > 3) { return "PROXY deep:1"; }
                return "PROXY proxy:3128; DIRECT";
            }
        "#;
        let run = |url: &str, host: &str| evaluate(script, url, host).expect("評価できる");
        assert_eq!(run("http://intranet/", "intranet"), "DIRECT");
        assert_eq!(run("http://a.corp.example/", "a.corp.example"), "DIRECT");
        assert_eq!(run("http://10.1.2.3/", "10.1.2.3"), "DIRECT");
        assert_eq!(
            run("https://raw.github.com/x", "raw.github.com"),
            "PROXY gh-proxy:8080"
        );
        assert_eq!(
            run("https://example.com/", "example.com"),
            "PROXY proxy:3128; DIRECT"
        );
        assert!(
            evaluate("function FindProxyForURL(", "u", "h").is_err(),
            "書き方の誤り"
        );
        assert!(!my_address().is_empty());
        assert_eq!(resolve("127.0.0.1").as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn local_scripts_are_read_and_cached() {
        let path = std::env::temp_dir().join(format!("mdview-pac-{}.pac", std::process::id()));
        std::fs::write(
            &path,
            "function FindProxyForURL(u, h) { return 'PROXY local:1'; }",
        )
        .unwrap();
        let url = format!("file://{}", path.display());
        assert_eq!(
            proxy_for(&url, "https://example.com/", "example.com").as_deref(),
            Some("http://local:1")
        );
        let _ = std::fs::remove_file(&path);
        // 消しても、覚えている間は同じ答え
        assert_eq!(
            proxy_for(&url, "https://example.com/", "example.com").as_deref(),
            Some("http://local:1")
        );
    }
}
