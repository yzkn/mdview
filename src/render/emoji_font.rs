//! 画面の絵文字を、同梱の字体（Noto Color Emoji）で描く（v3 R-19）。
//!
//! 画面の文字の部品（cosmic-text）は、本文の字体に無い字を OS の字体で補う（Windows は Segoe UI・Segoe UI Emoji・
//! **Segoe UI Symbol（色の無い絵文字の字形を持つ）**の順、macOS は Apple Color Emoji、Linux は入っている Noto Color Emoji など）。
//! そこで起動時に、
//!
//! 1. **OS の絵文字の字体を一覧から外す**（同じ名前の OS の字体と取り違えない）
//! 2. **補う先の並びの先頭に、同梱の絵文字の字体を置く**（Segoe UI Symbol の色の無い字形より先に拾う）
//!
//! 3. **同梱の絵文字の字体を、太さ違いとしても登録する**（補う先は太さがちょうど同じ字体しか選ばれない。
//!    絵文字の字体は普通の太さしか無いので、そのままだと見出し・太字の絵文字に色が付かない。実際に踏んだ）
//!
//! どの OS でも同じ絵文字の形になり、PDF・PPTX とそろう。絵文字の字体に無い記号は、これまでどおり OS の字体で補う

use iced::advanced::graphics::text::cosmic_text::{self, fontdb, Fallback, PlatformFallback};
use iced::advanced::graphics::text::font_system;

use crate::render::fonts::EMOJI_FAMILY;

/// 補う先の並び。**同梱の絵文字の字体を先頭に**置き、OS の絵文字の字体の名前は除く（ほかは OS の決まりのまま）
struct BundledEmoji {
    common: Vec<&'static str>,
}

impl BundledEmoji {
    fn new() -> Self {
        let common = std::iter::once(EMOJI_FAMILY)
            .chain(
                PlatformFallback
                    .common_fallback()
                    .iter()
                    .copied()
                    .filter(|name| !is_emoji_name(name)),
            )
            .collect();
        Self { common }
    }
}

impl Fallback for BundledEmoji {
    fn common_fallback(&self) -> &[&'static str] {
        &self.common
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        PlatformFallback.forbidden_fallback()
    }

    /// 文字の種類ごとの補う先は OS の決まりのまま（同梱の字体に無い文字のため）
    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        PlatformFallback.script_fallback(script, locale)
    }
}

/// 同梱の絵文字の字体を使う形に、画面の字体の一覧を作り直す（**窓を出す前に呼ぶ**。同梱の絵文字の字体もここで読む）。
/// 外した OS の字体の数を返す
pub fn use_bundled() -> usize {
    let Ok(mut system) = font_system().write() else {
        return 0;
    };
    let raw = system.raw();
    // 中身を取り出す（いったん空の一覧と入れ替える）
    let old = std::mem::replace(
        raw,
        cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(
            String::new(),
            fontdb::Database::new(),
            PlatformFallback,
        ),
    );
    let (locale, mut db) = old.into_locale_and_db();
    let emoji: Vec<_> = db
        .faces()
        .filter(|face| face.families.iter().any(|(name, _)| is_emoji_name(name)))
        .map(|face| face.id)
        .collect();
    for id in &emoji {
        db.remove_face(*id);
    }
    // 同梱の絵文字の字体を読み、太さ違いとしても載せる（中身は同じデータを指す）
    let source = fontdb::Source::Binary(std::sync::Arc::new(crate::render::fonts::EMOJI));
    for id in db.load_font_source(source) {
        let Some(face) = db.face(id).cloned() else {
            continue;
        };
        for weight in (100..=900)
            .step_by(100)
            .filter(|weight| *weight != face.weight.0)
        {
            db.push_face_info(fontdb::FaceInfo {
                weight: fontdb::Weight(weight),
                ..face.clone()
            });
        }
    }
    *raw = cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(
        locale,
        db,
        BundledEmoji::new(),
    );
    emoji.len()
}

/// `U+FE0F` 付きの字の区切りだけを絵文字の字体にした区切り（span）の並び（v3 R-19 C）。**無ければ `None`**（いつもの 1 つの字体で描く）。
///
/// 本文の字体がその字（`⚠` など）を持っていると、補う仕組みでは絵文字の字体にならないので、字体を名指しする。
/// 測る所（`IcedMeasurer`）と描く所（プレビュー）の両方で使う（**同じ分け方でないと、測った幅と描いた幅がずれる**）
pub fn presented_spans(
    content: &str,
    font: iced::Font,
) -> Option<Vec<iced::advanced::text::Span<'_, (), iced::Font>>> {
    let found = crate::render::emoji::presented(content);
    if found.is_empty() {
        return None;
    }
    let emoji = iced::Font::with_name(EMOJI_FAMILY);
    let mut spans = Vec::with_capacity(found.len() * 2 + 1);
    let mut at = 0;
    for range in found {
        if range.start > at {
            spans.push(iced::advanced::text::Span::new(&content[at..range.start]).font(font));
        }
        spans.push(iced::advanced::text::Span::new(&content[range.clone()]).font(emoji));
        at = range.end;
    }
    if at < content.len() {
        spans.push(iced::advanced::text::Span::new(&content[at..]).font(font));
    }
    Some(spans)
}

/// 絵文字の字体の名前か（`Emoji` を含む。Segoe UI Emoji・Apple Color Emoji・Noto Color Emoji・Twemoji など）
fn is_emoji_name(name: &str) -> bool {
    name.to_ascii_lowercase().contains("emoji")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_faces_are_found_by_name() {
        assert!(is_emoji_name("Segoe UI Emoji"));
        assert!(is_emoji_name("Twemoji Mozilla"));
        assert!(is_emoji_name("Apple Color Emoji"));
        assert!(!is_emoji_name("Segoe UI Symbol"));
    }

    /// 補う先の先頭が同梱の絵文字の字体で、OS の絵文字の字体の名前は入らない
    #[test]
    fn the_bundled_emoji_font_comes_first() {
        let fallback = BundledEmoji::new();
        assert_eq!(fallback.common_fallback()[0], EMOJI_FAMILY);
        assert_eq!(
            fallback
                .common_fallback()
                .iter()
                .filter(|name| is_emoji_name(name))
                .count(),
            1
        );
    }
}
