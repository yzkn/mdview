//! Marp の文書をスライドに分ける（v2.4.0 R-07 7a）。
//!
//! **描画も iced も知らない。** 入るのはブロックの並びと、その原文を引く口だけで、出るのは
//! 「どのブロックが何枚目のスライドの中身・背景・メモか」と、各スライドに効く指示。
//!
//! 決まり（Marpit に合わせる）:
//!
//! - 文書の頭の frontmatter（`---` で挟んだ所）に `marp: true` があるときだけ Marp の文書とみなす
//! - 罫線（`---` など）でスライドを分ける。frontmatter の 2 本の `---` は分け目にしない
//! - 指示は frontmatter と HTML のコメント（`<!-- paginate: true -->`）に書く。
//!   **全体の指示**（`theme`・`size`）はどこに書いても全部のスライドに効く。
//!   **その場の指示**（`paginate`・`header`・`footer`・`class`・`backgroundColor`・`color`）は、
//!   書いたスライドから後ろへ効く。頭に `_` を付けると、そのスライドだけに効く
//! - 指示でないコメントは発表者のメモ
//! - 中身が `![bg](…)` の画像だけの段落は、背景（1 枚・幾つか並べる・左右に寄せる）

pub mod assist;
pub mod check;
pub mod theme;

use std::ops::Range;

use crate::parse::{Block, BlockKind};

/// スライドの大きさ（Marp の `size`）。論理の画素
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideSize {
    #[default]
    Wide,
    Standard,
}

impl SlideSize {
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "4:3" | "4K:3" => Self::Standard,
            _ => Self::Wide,
        }
    }

    /// 幅と高さ（Marp と同じ論理の画素）
    pub fn dimensions(self) -> (f32, f32) {
        match self {
            Self::Wide => (1280.0, 720.0),
            Self::Standard => (960.0, 720.0),
        }
    }
}

/// 背景の画像の収め方（`![bg contain](…)` など）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    /// 隙間なく覆う（はみ出しは切る）。既定
    Cover,
    /// 全体が入るように収める（`contain`・`fit`）
    Contain,
    /// 原寸（`auto`）
    Auto,
    /// 原寸の何割か（`50%`）
    Scale(f32),
}

/// 背景の画像 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct Background {
    /// 書いたままの参照先（相対パスは文書の置き場から引く）
    pub reference: String,
    pub fit: Fit,
}

/// 背景を左右に寄せる（`![bg left](…)`）。画像が取る幅の割合
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Split {
    Left(f32),
    Right(f32),
}

/// 1 枚のスライドに効く指示
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlideStyle {
    pub theme: String,
    pub classes: Vec<String>,
    pub paginate: bool,
    pub header: Option<String>,
    pub footer: Option<String>,
    /// `#rrggbb` などの色（読めなければテーマの色）
    pub background_color: Option<[f32; 4]>,
    pub color: Option<[f32; 4]>,
}

/// 1 枚のスライド
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// このスライドに属するブロック（頭の分け目・コメント・背景を含む）
    pub range: Range<usize>,
    /// 中身として描くブロック
    pub content: Vec<usize>,
    /// 背景として描くブロック（`![bg](…)` の段落）と、その中の画像
    pub backgrounds: Vec<Background>,
    /// 背景を左右に寄せるとき（無ければスライド全体に敷く）
    pub split: Option<Split>,
    /// 背景を縦に並べるか（`![bg vertical](…)`）
    pub vertical: bool,
    pub style: SlideStyle,
    /// 発表者のメモ（指示でないコメント）
    pub notes: Vec<String>,
    /// 何枚目か（1 から）
    pub number: usize,
}

/// スライドに分けた文書
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    pub slides: Vec<Slide>,
    pub size: SlideSize,
}

impl Deck {
    /// このブロックが何枚目のスライドにあるか（0 から）
    pub fn slide_of(&self, block: usize) -> Option<usize> {
        let found = self
            .slides
            .partition_point(|slide| slide.range.end <= block);
        (found < self.slides.len() && self.slides[found].range.contains(&block)).then_some(found)
    }

    /// 中身として描くブロックか（コメント・背景・分け目・frontmatter は描かない）
    pub fn is_content(&self, block: usize) -> bool {
        self.slide_of(block)
            .is_some_and(|slide| self.slides[slide].content.contains(&block))
    }
}

/// 全体にだけ効く指示
const GLOBAL_KEYS: [&str; 4] = ["theme", "size", "headingDivider", "style"];
/// その場の指示（後ろへ効く。`_` で 1 枚だけ）
const LOCAL_KEYS: [&str; 10] = [
    "paginate",
    "header",
    "footer",
    "class",
    "backgroundColor",
    "color",
    "backgroundImage",
    "backgroundPosition",
    "backgroundRepeat",
    "backgroundSize",
];
/// frontmatter に書かれうる、指示でない鍵（メモにしない）
const META_KEYS: [&str; 7] = [
    "marp",
    "math",
    "title",
    "description",
    "author",
    "keywords",
    "image",
];

/// Marp の文書なら、スライドに分ける。`text_of` はブロックの原文を返す
pub fn deck_of(blocks: &[Block], text_of: impl Fn(&Block) -> String) -> Option<Deck> {
    let front_end = frontmatter_end(blocks, &text_of)?;
    let front: Vec<(String, String)> = blocks[1..front_end - 1]
        .iter()
        .flat_map(|block| pairs(&text_of(block)))
        .collect();
    let is_marp = front
        .iter()
        .any(|(key, value)| key == "marp" && value.trim() == "true");
    if !is_marp {
        return None;
    }

    // 1 本目の分け目までは frontmatter（1 枚目に含める）
    let mut starts = vec![0usize];
    for (index, block) in blocks.iter().enumerate().skip(front_end) {
        if block.kind == BlockKind::Rule {
            starts.push(index);
        }
    }

    let mut global = Global::default();
    let mut carried = SlideStyle {
        theme: String::new(),
        ..SlideStyle::default()
    };
    // frontmatter の指示はすべて 1 枚目から効く（`_` のものは 1 枚目だけ）
    let mut first_only = Vec::new();
    for (key, value) in &front {
        apply(&mut global, &mut carried, &mut first_only, key, value);
    }

    let mut slides = Vec::with_capacity(starts.len());
    for (number, start) in starts.iter().enumerate() {
        let end = starts.get(number + 1).copied().unwrap_or(blocks.len());
        let mut slide = Slide {
            range: *start..end,
            content: Vec::new(),
            backgrounds: Vec::new(),
            split: None,
            vertical: false,
            style: SlideStyle::default(),
            notes: Vec::new(),
            number: number + 1,
        };
        let mut only = if number == 0 {
            std::mem::take(&mut first_only)
        } else {
            Vec::new()
        };
        let from = if number == 0 { front_end } else { start + 1 };
        for (index, block) in blocks.iter().enumerate().take(end).skip(from) {
            if block.kind == BlockKind::Rule {
                continue;
            }
            if block.kind == BlockKind::Paragraph {
                let text = text_of(block);
                if let Some(comments) = comments_only(&text) {
                    for body in comments {
                        match directives(&body) {
                            Some(found) => {
                                for (key, value) in found {
                                    apply(&mut global, &mut carried, &mut only, &key, &value);
                                }
                            }
                            None => {
                                let note = body.trim();
                                if !note.is_empty() {
                                    slide.notes.push(note.to_owned());
                                }
                            }
                        }
                    }
                    continue;
                }
                if let Some(images) = backgrounds_only(&text) {
                    for image in images {
                        if image.vertical {
                            slide.vertical = true;
                        }
                        if image.split.is_some() {
                            slide.split = image.split;
                        }
                        slide.backgrounds.push(image.background);
                    }
                    continue;
                }
            }
            slide.content.push(index);
        }
        let mut style = carried.clone();
        for (key, value) in &only {
            set_local(&mut style, key, value);
        }
        slide.style = style;
        slides.push(slide);
    }
    for slide in &mut slides {
        slide.style.theme = global.theme.clone().unwrap_or_else(|| "default".to_owned());
    }
    Some(Deck {
        slides,
        size: global.size,
    })
}

#[derive(Default)]
struct Global {
    theme: Option<String>,
    size: SlideSize,
}

/// 指示を 1 つ当てる
fn apply(
    global: &mut Global,
    carried: &mut SlideStyle,
    only: &mut Vec<(String, String)>,
    key: &str,
    value: &str,
) {
    match key {
        "theme" => global.theme = Some(unquote(value)),
        "size" => global.size = SlideSize::parse(&unquote(value)),
        _ => {
            if let Some(local) = key.strip_prefix('_') {
                only.push((local.to_owned(), value.to_owned()));
            } else {
                set_local(carried, key, value);
            }
        }
    }
}

fn set_local(style: &mut SlideStyle, key: &str, value: &str) {
    let value = unquote(value);
    match key {
        "paginate" => style.paginate = value == "true",
        "header" => style.header = (!value.is_empty()).then_some(value),
        "footer" => style.footer = (!value.is_empty()).then_some(value),
        "class" => style.classes = value.split_whitespace().map(str::to_owned).collect(),
        "backgroundColor" => style.background_color = parse_color(&value),
        "color" => style.color = parse_color(&value),
        _ => {}
    }
}

/// 値を囲む引用符を外す
fn unquote(value: &str) -> String {
    let value = value.trim();
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

/// frontmatter の終わり（閉じの `---` の次のブロック）。**頭が `---` 1 行のときだけ**
fn frontmatter_end(blocks: &[Block], text_of: &impl Fn(&Block) -> String) -> Option<usize> {
    let first = blocks.first()?;
    if first.kind != BlockKind::Rule || first.start_line != 0 || text_of(first).trim() != "---" {
        return None;
    }
    let close = blocks
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, block)| block.kind == BlockKind::Rule)?
        .0;
    if text_of(&blocks[close]).trim() != "---" {
        return None;
    }
    Some(close + 1)
}

/// `key: value` の行を拾う（YAML の簡単な形だけ）
fn pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            (!key.is_empty() && !key.contains(' ')).then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

/// コメントの中身が指示だけなら、その鍵と値（ほかのものが混じればメモ）
fn directives(body: &str) -> Option<Vec<(String, String)>> {
    let mut found = Vec::new();
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line.split_once(':')?;
        let key = key.trim();
        let bare = key.strip_prefix('_').unwrap_or(key);
        let known = GLOBAL_KEYS.contains(&bare)
            || LOCAL_KEYS.contains(&bare)
            || (META_KEYS.contains(&bare) && !key.starts_with('_'));
        if !known {
            return None;
        }
        found.push((key.to_owned(), value.to_owned()));
    }
    (!found.is_empty()).then_some(found)
}

/// 段落がコメントだけなら、その中身（幾つでも）
fn comments_only(text: &str) -> Option<Vec<String>> {
    let mut rest = text.trim();
    if !rest.starts_with("<!--") {
        return None;
    }
    let mut bodies = Vec::new();
    while !rest.is_empty() {
        let inner = rest.strip_prefix("<!--")?;
        let close = inner.find("-->")?;
        bodies.push(inner[..close].to_owned());
        rest = inner[close + 3..].trim_start();
    }
    Some(bodies)
}

struct BackgroundImage {
    background: Background,
    split: Option<Split>,
    vertical: bool,
}

/// 段落が `![bg …](…)` の画像だけなら、その画像（幾つでも）
fn backgrounds_only(text: &str) -> Option<Vec<BackgroundImage>> {
    let mut rest = text.trim();
    if !rest.starts_with("![") {
        return None;
    }
    let mut found = Vec::new();
    while !rest.is_empty() {
        let inner = rest.strip_prefix("![")?;
        let close = inner.find("](")?;
        let alt = &inner[..close];
        let after = &inner[close + 2..];
        let end = after.find(')')?;
        let target = after[..end].trim();
        // 題（`"…"`）は外す
        let reference = target.split_whitespace().next()?.to_owned();
        let mut words = alt.split_whitespace();
        if words.next() != Some("bg") {
            return None;
        }
        let mut image = BackgroundImage {
            background: Background {
                reference,
                fit: Fit::Cover,
            },
            split: None,
            vertical: false,
        };
        for word in words {
            let (name, amount) = match word.split_once(':') {
                Some((name, amount)) => (name, percent(amount)),
                None => (word, None),
            };
            match name {
                "contain" | "fit" => image.background.fit = Fit::Contain,
                "cover" => image.background.fit = Fit::Cover,
                "auto" => image.background.fit = Fit::Auto,
                "left" => image.split = Some(Split::Left(amount.unwrap_or(0.5))),
                "right" => image.split = Some(Split::Right(amount.unwrap_or(0.5))),
                "vertical" => image.vertical = true,
                _ => {
                    if let Some(scale) = percent(name) {
                        image.background.fit = Fit::Scale(scale);
                    }
                }
            }
        }
        found.push(image);
        rest = after[end + 1..].trim_start();
    }
    Some(found)
}

/// `50%` を 0.5 に
fn percent(text: &str) -> Option<f32> {
    let number: f32 = text.strip_suffix('%')?.parse().ok()?;
    (number > 0.0).then_some(number / 100.0)
}

/// `#rgb`・`#rrggbb`・`#rrggbbaa`・よく使う色の名前
pub fn parse_color(text: &str) -> Option<[f32; 4]> {
    let text = text.trim();
    let named = match text.to_ascii_lowercase().as_str() {
        "white" => Some("#ffffff"),
        "black" => Some("#000000"),
        "red" => Some("#ff0000"),
        "green" => Some("#008000"),
        "blue" => Some("#0000ff"),
        "yellow" => Some("#ffff00"),
        "gray" | "grey" => Some("#808080"),
        "orange" => Some("#ffa500"),
        "navy" => Some("#000080"),
        "teal" => Some("#008080"),
        "purple" => Some("#800080"),
        "transparent" => Some("#00000000"),
        _ => None,
    };
    let hex = named.unwrap_or(text).strip_prefix('#')?;
    let digit = |index: usize, width: usize| -> Option<f32> {
        let part = hex.get(index * width..index * width + width)?;
        let value = u8::from_str_radix(part, 16).ok()?;
        Some(if width == 1 {
            f32::from(value * 17) / 255.0
        } else {
            f32::from(value) / 255.0
        })
    };
    match hex.len() {
        3 => Some([digit(0, 1)?, digit(1, 1)?, digit(2, 1)?, 1.0]),
        6 => Some([digit(0, 2)?, digit(1, 2)?, digit(2, 2)?, 1.0]),
        8 => Some([digit(0, 2)?, digit(1, 2)?, digit(2, 2)?, digit(3, 2)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(text: &str) -> Option<Deck> {
        let blocks = crate::parse::scan_lines(text).blocks;
        deck_of(&blocks, |block| text[block.bytes.clone()].to_owned())
    }

    #[test]
    fn only_marp_documents_are_split() {
        assert!(deck("# 題\n\n---\n\n次\n").is_none(), "frontmatter が無い");
        assert!(
            deck("---\ntitle: x\n---\n\n# 題\n").is_none(),
            "marp: true が無い"
        );
        let found =
            deck("---\nmarp: true\n---\n\n# 1 枚目\n\n---\n\n# 2 枚目\n\n***\n\n3\n").unwrap();
        assert_eq!(found.slides.len(), 3);
        assert_eq!(found.slides[0].number, 1);
        assert_eq!(found.slides[0].style.theme, "default");
        assert_eq!(found.size, SlideSize::Wide);
    }

    #[test]
    fn content_skips_separators_frontmatter_and_comments() {
        let text = "---\nmarp: true\n---\n\n# 1\n\n<!-- メモです -->\n\n---\n\n本文\n";
        let found = deck(text).unwrap();
        let blocks = crate::parse::scan_lines(text).blocks;
        let kinds: Vec<_> = found.slides[0]
            .content
            .iter()
            .map(|index| blocks[*index].kind.clone())
            .collect();
        assert_eq!(kinds, vec![BlockKind::Heading(1)]);
        assert_eq!(found.slides[0].notes, vec!["メモです".to_owned()]);
        assert_eq!(found.slides[1].content.len(), 1);
        // ブロックはどれかのスライドに入る（抜けも重なりも無い）
        assert_eq!(found.slides[0].range.start, 0);
        assert_eq!(found.slides[0].range.end, found.slides[1].range.start);
        assert_eq!(found.slides[1].range.end, blocks.len());
        assert_eq!(found.slide_of(blocks.len() - 1), Some(1));
        assert!(!found.is_content(0));
    }

    #[test]
    fn directives_carry_forward_unless_underscored() {
        let text = "---\nmarp: true\ntheme: gaia\npaginate: true\nheader: '見出し'\n---\n\n1\n\n---\n\n<!-- _class: lead -->\n<!-- backgroundColor: #123 -->\n\n2\n\n---\n\n3\n\n<!-- paginate: false -->\n";
        let found = deck(text).unwrap();
        let [a, b, c] = &found.slides[..] else {
            panic!("3 枚")
        };
        assert_eq!(a.style.theme, "gaia");
        assert!(a.style.paginate && b.style.paginate);
        assert!(!c.style.paginate, "そのスライドから後ろへ効く");
        assert_eq!(a.style.header.as_deref(), Some("見出し"));
        assert_eq!(b.style.classes, vec!["lead".to_owned()]);
        assert!(c.style.classes.is_empty(), "_ は 1 枚だけ");
        assert_eq!(
            b.style.background_color,
            Some([
                0x11 as f32 / 255.0,
                0x22 as f32 / 255.0,
                0x33 as f32 / 255.0,
                1.0
            ])
        );
        assert_eq!(c.style.background_color, b.style.background_color);
        assert!(a.style.background_color.is_none());
    }

    #[test]
    fn global_directives_apply_everywhere() {
        let found = deck(
            "---\nmarp: true\n---\n\n1\n\n---\n\n<!-- theme: uncover -->\n<!-- size: 4:3 -->\n",
        )
        .unwrap();
        assert!(found
            .slides
            .iter()
            .all(|slide| slide.style.theme == "uncover"));
        assert_eq!(found.size, SlideSize::Standard);
    }

    #[test]
    fn background_images_are_not_content() {
        let text = "---\nmarp: true\n---\n\n![bg left:40% contain](a.png)\n\n# 題\n\n---\n\n![bg](b.png)\n![bg 50%](c.png)\n\n![絵](d.png)\n";
        let found = deck(text).unwrap();
        let first = &found.slides[0];
        assert_eq!(first.split, Some(Split::Left(0.4)));
        assert_eq!(first.backgrounds[0].fit, Fit::Contain);
        assert_eq!(first.backgrounds[0].reference, "a.png");
        assert_eq!(first.content.len(), 1);
        let second = &found.slides[1];
        assert_eq!(second.backgrounds.len(), 2);
        assert_eq!(second.backgrounds[1].fit, Fit::Scale(0.5));
        assert_eq!(second.content.len(), 1, "bg でない画像は中身");
    }

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_color("white"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(
            parse_color("#00000080").map(|c| c[3] > 0.49 && c[3] < 0.51),
            Some(true)
        );
        assert_eq!(parse_color("rgb(1,2,3)"), None);
    }
}
