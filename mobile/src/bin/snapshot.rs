//! 画面の確認（v3 R-02）: 決まった様子の画面を HTML にする。**配る物には入らない**。
//!
//! `snapshot <様子> <文書> <出力>`。様子は read・toc・find・edit・block・full・view・more（画面の HTML）と pdf・html（出力）。
//! 出てきた HTML をヘッドレスのブラウザで端末の大きさに撮る（画面の側の仕掛け bridge.js は動かない）
use dioxus::prelude::*;
use mdview_mobile::{Sheet, Start, START};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(kind), Some(source), Some(out)) = (args.next(), args.next(), args.next()) else {
        eprintln!("使い方: snapshot <様子> <文書> <出力.html>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(&source).expect("文書を読めません");
    // PDF・HTML の出力も見る（字の幅を同梱の字体で測った割り付け）
    if let Some(format) = match kind.as_str() {
        "pdf" => Some(mdview_mobile::platform::Format::Pdf),
        "html" => Some(mdview_mobile::platform::Format::Html),
        _ => None,
    } {
        let bytes = mdview_mobile::output::export(&text, "sample", format).expect("出力できる");
        std::fs::write(&out, bytes).expect("書けません");
        return;
    }
    let mut start = Start {
        name: Some(
            std::path::Path::new(&source)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        text: Some(text),
        ..Start::default()
    };
    match kind.as_str() {
        "read" => {}
        "toc" => start.sheet = Some(Sheet::Toc),
        "find" => {
            start.sheet = Some(Sheet::Find);
            start.query = Some("mdview".into());
        }
        "edit" => start.edit_mode = true,
        "block" => {
            start.edit_mode = true;
            start.block = Some(2);
        }
        "full" => start.sheet = Some(Sheet::Full),
        "view" => start.sheet = Some(Sheet::View),
        "more" => start.sheet = Some(Sheet::More),
        other => panic!("知らない様子: {other}"),
    }
    START.set(start).expect("1 度だけ決める");
    let mut dom = VirtualDom::new(mdview_mobile::App);
    dom.rebuild_in_place();
    let body = initial_values(&dioxus::ssr::render(&dom));
    let page = format!(
        "<!DOCTYPE html><html lang=\"ja\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"></head><body>{body}</body></html>"
    );
    std::fs::write(&out, page).expect("書けません");
}

/// SSR は `initial_value` を属性のまま書く（WebView では Dioxus が 1 度だけ値を入れる）。撮るために、
/// 入力欄は `value`、`<textarea>` は中身にする
fn initial_values(html: &str) -> String {
    const MARK: &str = " initial_value=\"";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(MARK) {
        let tag_start = rest[..at].rfind('<').unwrap_or(0);
        let value_start = at + MARK.len();
        let value_end = value_start + rest[value_start..].find('"').unwrap_or(0);
        let value = &rest[value_start..value_end];
        if rest[tag_start..].starts_with("<textarea") {
            let close = value_end + rest[value_end..].find('>').unwrap_or(0);
            out.push_str(&rest[..at]);
            out.push_str(&rest[value_end + 1..=close]);
            out.push_str(value);
            rest = &rest[close + 1..];
        } else {
            out.push_str(&rest[..at]);
            out.push_str(" value=\"");
            out.push_str(value);
            out.push('"');
            rest = &rest[value_end + 1..];
        }
    }
    out.push_str(rest);
    out
}
