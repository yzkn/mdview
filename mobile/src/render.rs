//! ブロックごとの HTML（v3 R-02）。**直したブロックだけを描き直す**（全文の書き出しは 300 KB で約 1 秒かかる）。
//!
//! 鍵はブロックの原文そのもの（の指紋）。位置がずれただけのブロックは描き直さない

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use mdview_core::export::html;

#[derive(Default)]
pub struct Cache {
    map: HashMap<u64, Rc<str>>,
    /// 描いたときの描き方の設定（v3.1 R-08。変わったら描き直す）
    key: String,
}

impl Cache {
    /// ブロックの HTML（無ければ描く）
    pub fn html(&mut self, source: &str) -> Rc<str> {
        let key = fingerprint(source);
        self.map
            .entry(key)
            .or_insert_with(|| {
                Rc::from(code_html(source).unwrap_or_else(|| html::fragment(source, None)))
            })
            .clone()
    }

    /// 描き方の設定が変わっていたら、描いたものを捨てる
    pub fn sync(&mut self, key: &str) {
        if self.key != key {
            self.map.clear();
            self.key = key.to_owned();
        }
    }

    /// 使わなくなったものを捨てる（文書を開き直したとき・多くなりすぎたとき）
    pub fn trim(&mut self, alive: usize) {
        if self.map.len() > alive.saturating_mul(4).max(512) {
            self.map.clear();
        }
    }
}

/// Marp のスライド（v3.1 R-10）。PC 版と同じく、スライドの枠を並べて見せる
pub struct Slides {
    /// 1 枚ずつの区画（土台の `export::slides::parts`）
    pub slides: Vec<Rc<str>>,
    pub css: Rc<str>,
    /// ブロックの番号 → 何枚目か（目次で飛ぶ）
    pub slide_of: Vec<Option<usize>>,
}

/// Marp の文書ならスライドに分けて描く（そうでなければ `None`）
pub fn slides(text: &str) -> Option<Slides> {
    use mdview_core::document::Document;
    use mdview_core::export::{slides, Silent};
    let document = Document::from_text(text.to_owned());
    let deck = mdview_core::marp::deck_of(document.blocks(), |block| {
        document.text().byte_slice(block.bytes.clone()).to_string()
    })?;
    let parts = slides::parts(&document, &deck, None, &Silent).ok()?;
    Some(Slides {
        slides: parts.slides.into_iter().map(Rc::from).collect(),
        css: Rc::from(parts.css),
        slide_of: (0..document.blocks().len())
            .map(|block| deck.slide_of(block))
            .collect(),
    })
}

/// 囲みのコードに色を付ける（v3.1 R-04。PC 版のプレビューと同じ）。書き出しの HTML は色を付けないので、ここで作る。
///
/// 字を土台の色付け（`parse::highlight`）で役割に分け、役割の印（`tk-kw` など）を付ける。色は `theme.rs` の CSS。
/// 図（mermaid）・数式は書き出しに任せる（`None`）
pub fn code_html(source: &str) -> Option<String> {
    use mdview_core::parse::highlight::{highlight, TokenRole};

    let (first, rest) = source.split_once('\n').unwrap_or((source, ""));
    let line = first.trim_start_matches(' ');
    if first.len() - line.len() > 3 {
        return None;
    }
    let mark = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let fence = line.chars().take_while(|c| *c == mark).count();
    if fence < 3 {
        return None;
    }
    let info = line[fence..].trim();
    let language = info.split_whitespace().next().unwrap_or("");
    if matches!(
        language.to_ascii_lowercase().as_str(),
        "mermaid" | "math" | "latex" | "katex"
    ) {
        return None;
    }
    // 閉じの囲みを外す（無ければ最後まで）
    let closing = |l: &str| {
        let t = l.trim();
        t.len() >= fence && t.chars().all(|c| c == mark)
    };
    let mut body: Vec<&str> = rest.split_inclusive('\n').collect();
    if body.last().is_some_and(|l| closing(l)) {
        body.pop();
    }
    let body = body.concat();
    let lines = highlight(&body, (!language.is_empty()).then_some(language));

    let mut out = String::from("<pre><code");
    if !language.is_empty() {
        out.push_str(&format!(" class=\"language-{}\"", escape(language)));
    }
    out.push('>');
    for (index, text) in body.split_inclusive('\n').enumerate() {
        let mut at = 0;
        // 色付けの区間が無い行・字の境目に無い区間は、素の字のまま出す（字を落とさない）
        for token in lines.get(index).map(Vec::as_slice).unwrap_or_default() {
            let start = token.range.start.clamp(at, text.len());
            let end = token.range.end.clamp(start, text.len());
            let (Some(before), Some(inner)) = (text.get(at..start), text.get(start..end)) else {
                continue;
            };
            out.push_str(&escape(before));
            let piece = escape(inner);
            let class = match token.role {
                TokenRole::Keyword => Some("tk-kw"),
                TokenRole::Str => Some("tk-str"),
                TokenRole::Comment => Some("tk-com"),
                TokenRole::Number | TokenRole::Constant => Some("tk-num"),
                TokenRole::Type => Some("tk-type"),
                TokenRole::Function => Some("tk-fn"),
                TokenRole::Plain | TokenRole::Punctuation => None,
            };
            match class {
                Some(class) => out.push_str(&format!("<span class=\"{class}\">{piece}</span>")),
                None => out.push_str(&piece),
            }
            at = end.max(at);
        }
        out.push_str(&escape(text.get(at..).unwrap_or_default()));
    }
    out.push_str("</code></pre>\n");
    Some(out)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn fingerprint(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_drawn_once() {
        let mut cache = Cache::default();
        let first = cache.html("# 題\n");
        assert!(first.contains("<h1"), "{first}");
        let again = cache.html("# 題\n");
        assert!(Rc::ptr_eq(&first, &again), "同じ原文は描き直さない");
        assert!(cache.html("- [ ] 作業\n").contains("checkbox"));
    }

    /// Marp の文書はスライドの枠に分ける。見出しのブロックから何枚目かを引ける（v3.1 R-10）
    #[test]
    fn marp_documents_become_slides() {
        let text = "---\nmarp: true\n---\n\n# 一枚目\n\n本文\n\n---\n\n# 二枚目\n";
        let slides = slides(text).expect("Marp の文書");
        assert_eq!(slides.slides.len(), 2);
        assert!(slides.slides[1].contains("二枚目"));
        // 区画の CSS は画面の body に触らない（書き出しの HTML だけが足す）
        assert!(slides.css.contains(".mv-slide") && !slides.css.contains("body { margin"));
        let pieces = crate::doc::pieces(text);
        let second = pieces
            .iter()
            .position(|piece| text[piece.range.clone()].contains("二枚目"))
            .unwrap();
        assert_eq!(slides.slide_of.get(second).copied().flatten(), Some(1));
        // 普通の文書はスライドにしない
        assert!(super::slides("# 題\n\n本文\n").is_none());
    }

    /// 囲みのコードは PC 版のプレビューと同じく役割ごとに色の印を付ける。字は落とさない（v3.1 R-04）
    #[test]
    fn code_blocks_are_colored() {
        let html = code_html("```rust\nfn main() { let s = \"<a>\"; }\n```\n").expect("コード");
        assert!(html.contains("class=\"language-rust\""), "{html}");
        assert!(html.contains("<span class=\"tk-kw\">"), "{html}");
        assert!(html.contains("&lt;a&gt;"), "{html}");
        let plain: String = html
            .split('<')
            .map(|part| part.split_once('>').map_or(part, |(_, rest)| rest))
            .collect();
        assert!(
            plain.contains("fn main() { let s = &quot;&lt;a&gt;&quot;; }"),
            "{plain}"
        );
        // 知らない言語・言語無しでも字はそのまま
        assert!(code_html("~~~\n1 < 2\n~~~\n").unwrap().contains("1 &lt; 2"));
        // 図・数式・コードでないものは書き出しに任せる
        assert!(code_html("```mermaid\ngraph TD\n```\n").is_none());
        assert!(code_html("# 題\n").is_none());
    }
}
