//! `$$ ... $$` を ```` ```math ```` へ書き換える（§16.6）。
//!
//! # なぜ要るのか
//!
//! 画面と PDF は自前の走査器（[`super::scan`]）を通すので、`$$` を
//! 数式として扱える。**HTML 出力だけは comrak が解析する**ため、
//! comrak の知らない `$$` はただの段落になってしまう。
//!
//! # なぜ「書き換え」なのか
//!
//! **`$$` が数式かどうかを決める場所を増やさない。** ここで新しく
//! 判定を書くと、画面と HTML で食い違う余地ができる（§10.28 の形）。
//! 走査器に聞いて、その答えのとおりに書き換えるだけにする。

use std::borrow::Cow;

use super::scan::{scan_lines, BlockKind};

/// `$$` の囲みを ```` ```math ```` へ置き換える。
///
/// 置き換えるものが無ければ、**写さずにそのまま返す**（10MB で複製しない）。
pub fn to_math_fences(source: &str) -> Cow<'_, str> {
    let blocks = scan_lines(source).blocks;

    // **先に当たりを付ける。** ほとんどの文書には `$$` が無い
    let targets: Vec<_> = blocks
        .iter()
        .filter(|block| {
            matches!(&block.kind, BlockKind::Code { language } if language.as_deref() == Some("math"))
                && source[block.bytes.clone()].trim_start().starts_with("$$")
        })
        .collect();

    if targets.is_empty() {
        return Cow::Borrowed(source);
    }

    let mut out = String::with_capacity(source.len() + targets.len() * 8);
    let mut at = 0usize;

    for block in targets {
        out.push_str(&source[at..block.bytes.start]);

        let body = &source[block.bytes.clone()];
        let mut lines = body.lines();
        // 1 行目は `$$`、最後の行も `$$`。**中身だけを挟み直す**
        let _open = lines.next();
        let inner: Vec<&str> = lines.collect();
        let inner = match inner.split_last() {
            Some((_close, rest)) => rest,
            None => &[],
        };

        out.push_str("```math\n");
        for line in inner {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("```\n");

        at = block.bytes.end;
    }
    out.push_str(&source[at..]);

    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **`$$` を数式の囲みへ直す。**
    #[test]
    fn a_dollar_block_becomes_a_math_fence() {
        let out = to_math_fences("$$\nE = mc^2\n$$\n");
        assert_eq!(out, "```math\nE = mc^2\n```\n");
    }

    /// 前後はそのまま残る。
    #[test]
    fn the_text_around_it_is_kept() {
        let out = to_math_fences("前\n\n$$\nx\n$$\n\n後\n");
        assert_eq!(out, "前\n\n```math\nx\n```\n\n後\n");
    }

    /// 複数あっても直す。
    #[test]
    fn several_blocks_are_converted() {
        let out = to_math_fences("$$\na\n$$\n\n$$\nb\n$$\n");
        assert_eq!(out, "```math\na\n```\n\n```math\nb\n```\n");
    }

    /// 中身が複数行でも保つ。
    #[test]
    fn a_multiline_body_is_kept() {
        let out = to_math_fences("$$\n\\begin{aligned}\nx &= 1\n\\end{aligned}\n$$\n");
        assert_eq!(
            out,
            "```math\n\\begin{aligned}\nx &= 1\n\\end{aligned}\n```\n"
        );
    }

    /// **コードの中の `$$` は触らない。** 走査器が囲みの中と判断する
    #[test]
    fn a_dollar_inside_a_code_fence_is_left_alone() {
        let source = "```sh\n$$\necho\n$$\n```\n";
        assert_eq!(to_math_fences(source), source);
    }

    /// **何も無ければ写さない**（10MB で複製しない）。
    #[test]
    fn a_document_without_dollars_is_not_copied() {
        let source = "普通の段落\n\n```rust\nfn main() {}\n```\n";
        assert!(matches!(to_math_fences(source), Cow::Borrowed(_)));
    }

    /// ```` ```math ```` は元から正しいので触らない。
    #[test]
    fn an_existing_math_fence_is_left_alone() {
        let source = "```math\nx\n```\n";
        assert_eq!(to_math_fences(source), source);
    }

    /// `$` 1 つは触らない（金額の表記を壊さない）。
    #[test]
    fn a_single_dollar_is_left_alone() {
        let source = "$100 と $200\n";
        assert_eq!(to_math_fences(source), source);
    }
}
