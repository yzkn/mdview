//! 同梱の字体を WebView で使う（v3.1 R-03）。
//!
//! - 字体の中身は**土台が抱えているもの**（`fonts::EMBEDDED`・`fonts::EMOJI`。PDF の書き出しのため、もう APK に入っている）を
//!   そのまま返す。写さないので APK もメモリも増えない
//! - WebView からは `/mvfont/<名前>` で取る（Dioxus の `use_asset_handler`）
//! - `@font-face` の名前は、**書き出しの CSS（`export::html::stylesheet`）が使っている名前**にそろえる。本文は IBM Plex Sans JP・
//!   コードは PlemolJP・絵文字は Noto Color Emoji（PC 版と同じ使い分け。`fonts::for_style`）

use mdview_core::render::fonts;

/// `asset_handler` の名前（URL の 1 段目）
pub const HANDLER: &str = "mvfont";

/// `/mvfont/<名前>` の中身
pub fn bytes(name: &str) -> Option<&'static [u8]> {
    let [mono, mono_bold, body, body_bold] = fonts::EMBEDDED;
    Some(match name {
        "body.ttf" => body,
        "body-bold.ttf" => body_bold,
        "mono.ttf" => mono,
        "mono-bold.ttf" => mono_bold,
        "emoji.ttf" => fonts::EMOJI,
        _ => return None,
    })
}

/// 同梱の字体の `@font-face`。絵文字は本文・コードの字体に無い字のときに使う（後ろに並べる）
pub fn face_css() -> String {
    let face = |family: &str, file: &str, weight: u16| {
        format!(
            "@font-face {{ font-family: \"{family}\"; src: url(\"/{HANDLER}/{file}\") format(\"truetype\"); \
             font-weight: {weight}; font-display: swap; }}\n"
        )
    };
    let emoji = fonts::EMOJI_FAMILY;
    [
        face("IBM Plex Sans JP", "body.ttf", 400),
        face("IBM Plex Sans JP", "body-bold.ttf", 700),
        face("PlemolJP", "mono.ttf", 400),
        face("PlemolJP", "mono-bold.ttf", 700),
        face(emoji, "emoji.ttf", 400),
        // 本文・板・帯も同じ字体で（書き出しの CSS は本文だけを決めている）。絵文字は後ろ
        format!(
            "body {{ font-family: \"IBM Plex Sans JP\", \"{emoji}\", sans-serif; font-variant-emoji: unicode; }}\n\
             #read main {{ font-family: \"IBM Plex Sans JP\", \"{emoji}\", sans-serif; }}\n\
             #read code, #read pre, textarea {{ font-family: \"PlemolJP\", \"{emoji}\", monospace; }}\n"
        ),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_has_bytes() {
        let css = face_css();
        for file in [
            "body.ttf",
            "body-bold.ttf",
            "mono.ttf",
            "mono-bold.ttf",
            "emoji.ttf",
        ] {
            assert!(css.contains(&format!("/{HANDLER}/{file}")), "{file}");
            assert!(bytes(file).is_some_and(|b| b.len() > 1000), "{file}");
        }
        assert!(bytes("../secret").is_none());
        // 書き出しの CSS と同じ名前（そろっていないと、書き出しの CSS の字体が端末の字体で描かれる）
        let export = mdview_core::export::html::stylesheet();
        assert!(export.contains("\"IBM Plex Sans JP\"") && export.contains("\"PlemolJP\""));
    }
}
