//! 同梱フォント（§16.11 / DEC-209）。
//!
//! **システムフォントへのフォールバックを行わない。**
//! 漢字統合により、日本語の文書に中国語の字形が拾われるためである
//! （§6.5.5 で実際に「入」の字で確認した）。
//!
//! 代わりに 2 書体を実行ファイルへ埋め込む。PlemolJP の和文部分は
//! IBM Plex Sans JP そのものなので、**エディタとプレビューで漢字の字形が一致する**。
//!
//! フォント本体はリポジトリに無い。`python tools/fetch-fonts.py` で用意する。

use crate::layout::TextStyle;

/// エディタ・コードブロック。等幅（半角:全角 = 1:2）。
pub const MONO: &str = "PlemolJP";

/// 本文・見出し・UI。プロポーショナル。
pub const BODY: &str = "IBM Plex Sans JP";

/// 起動時に読み込むフォント。
///
/// **Regular と Bold を別々に渡す。** cosmic-text は同名の書体族として扱い、
/// `Weight` で選び分ける。イタリックは同梱しない（DEC-209）。
pub const EMBEDDED: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/PlemolJP-Regular.ttf"),
    include_bytes!("../../assets/fonts/PlemolJP-Bold.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSansJP-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSansJP-Bold.ttf"),
];

/// ライセンス全文。**配布物に含める義務がある**（§7.6）。
///
/// 画面に出す口は P4（About ダイアログ）で作る。それまでも実行ファイルには
/// 入れておく——後から入れ忘れるのを防ぐためである。
pub const LICENSES: [(&str, &str); 2] = [
    (MONO, include_str!("../../assets/fonts/PlemolJP-OFL.txt")),
    (
        BODY,
        include_str!("../../assets/fonts/IBMPlexSansJP-OFL.txt"),
    ),
];

/// 用途から書体を決める。
///
/// **`Font::DEFAULT` を使わない。** 既定はシステムフォントであり、
/// それを使った時点で漢字統合の問題が戻る。
pub fn body() -> iced::Font {
    iced::Font::with_name(BODY)
}

pub fn body_bold() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Bold,
        ..body()
    }
}

pub fn mono() -> iced::Font {
    iced::Font::with_name(MONO)
}

/// 見た目から書体と大きさを決める。
///
/// **描画と測定はここを共有する。** 別々に持つと、測った幅と描いた幅が
/// 食い違い、折り返し位置とキャレット位置がずれる。
pub fn for_style(style: TextStyle, base: f32) -> (iced::Font, f32) {
    match style {
        // コードは本文よりわずかに小さく。等幅は同じ字数でも幅が出るため
        TextStyle::Mono => (mono(), base * 0.9),
        TextStyle::Bold => (body_bold(), base),
        TextStyle::Heading(level) => (
            body_bold(),
            base * crate::layout::style_scale(TextStyle::Heading(level)),
        ),
        TextStyle::Body => (body(), base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4 本とも埋め込まれ、中身がある。
    #[test]
    fn all_fonts_are_embedded() {
        for bytes in EMBEDDED {
            // TrueType の署名。取り違えると字が出ないだけで気づきにくい
            assert_eq!(&bytes[..4], b"\x00\x01\x00\x00", "TTF ではない");
            assert!(bytes.len() > 4_000_000, "小さすぎる: {}", bytes.len());
        }
    }

    /// ライセンス全文が入っている（配布の義務。§7.6）。
    #[test]
    fn licenses_are_embedded() {
        for (name, text) in LICENSES {
            assert!(
                text.contains("SIL OPEN FONT LICENSE"),
                "{name} の OFL 全文が入っていない"
            );
        }
    }

    /// 既定（システムフォント）を使っていない。漢字統合の問題が戻るため。
    #[test]
    fn never_falls_back_to_system_font() {
        assert_ne!(body(), iced::Font::DEFAULT);
        assert_ne!(mono(), iced::Font::DEFAULT);
        assert_ne!(mono(), iced::Font::MONOSPACE);
    }

    /// **どの見た目でも同梱フォントを返す。** 1 つでも既定に落ちると、
    /// そこだけ中国語の字形が出る。
    #[test]
    fn every_style_uses_a_bundled_font() {
        for style in [
            TextStyle::Body,
            TextStyle::Bold,
            TextStyle::Mono,
            TextStyle::Heading(1),
            TextStyle::Heading(6),
        ] {
            let (font, size) = for_style(style, 16.0);
            assert_ne!(font, iced::Font::DEFAULT, "{style:?} が既定に落ちた");
            assert_ne!(font, iced::Font::MONOSPACE, "{style:?} が既定に落ちた");
            assert!(size > 0.0);
        }
    }

    /// 見出しは本文より大きく、コードは本文より小さい。
    #[test]
    fn sizes_are_ordered() {
        let body_size = for_style(TextStyle::Body, 16.0).1;
        assert!(for_style(TextStyle::Heading(1), 16.0).1 > body_size);
        assert!(for_style(TextStyle::Mono, 16.0).1 < body_size);
    }

    /// 太字と見出しは Bold のウェイトになる。
    #[test]
    fn bold_styles_use_bold_weight() {
        for style in [TextStyle::Bold, TextStyle::Heading(2)] {
            assert_eq!(
                for_style(style, 16.0).0.weight,
                iced::font::Weight::Bold,
                "{style:?}"
            );
        }
    }
}
