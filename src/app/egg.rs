//! イースターエッグ（v2.5.0 R-04）。About ダイアログの決まった所を操作すると出る。**画面には書かない。**
//!
//! - アプリの名前を 5 回押す → ひとこと（押すたび・日ごとに替わる）
//! - 版数を 3 回押す → 絵（アイコンが跳ね、Markdown の記号が降る）
//! - カードの右下の隅をダブルクリック → ミニゲーム（記号を集めるヘビ。矢印で動かす。Enter でもう一度）
//! - About を開いたまま ↑↑↓↓←→←→BA → 作った記録（CHANGELOG の版ごとの歩みが流れる）
//!
//! どれも Esc で About に戻る。ネットにはつながない。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::{self, Text};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{image, renderer};
use iced::{mouse, Color, Element, Length, Pixels, Point, Rectangle, Size};

use super::App;
use crate::render::{EggKey, EggSpot, Message};

/// 出しているもの
#[derive(Debug, Clone)]
pub(super) enum Egg {
    /// ひとこと（何番目か）
    Word(usize),
    /// 絵（出した時刻）
    Picture(Instant),
    /// ミニゲーム
    Game(Snake),
    /// 作った記録（出した時刻）
    Roll(Instant),
}

/// 押した回数など（About を閉じると忘れる）
#[derive(Debug, Clone, Default)]
pub(super) struct EggState {
    pub egg: Option<Egg>,
    title: u8,
    version: u8,
    konami: usize,
    words: usize,
}

/// ひとこと
const WORDS: [&str; 12] = [
    "作った人より: 読みやすい文章が、いちばんの機能です。",
    "今日もよい文章を。",
    "# は見出し、* は強調、` はコード。それだけ覚えれば、だいたい書けます。",
    "1 つの窓で、1 つのファイル。迷わないように。",
    "保存は Ctrl+S。……もう知っていましたか。",
    "10 MB の文書でも、すぐに開けるように作りました。",
    "ここを 5 回押す人は、きっと細かいところまで読む人です。",
    "書き終えたら、Ctrl+Alt+F で整えてみてください。",
    "見出しは 1 段ずつ深く。文章も、1 歩ずつ。",
    "ネットにつながなくても、ちゃんと動きます。",
    "おつかれさまです。お茶でもどうぞ。",
    "隠しものは、まだあります。",
];

/// ゲームの盤の大きさ（マス）
pub(super) const COLUMNS: i32 = 26;
pub(super) const ROWS: i32 = 13;
/// ゲームの記号
const SYMBOLS: [char; 6] = ['#', '*', '-', '`', '>', '|'];

/// 向き
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Heading {
    Up,
    Down,
    Left,
    Right,
}

impl Heading {
    fn step(self) -> (i32, i32) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// 記号を集めるヘビ（**画面を知らない**。1 歩ずつ進める）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Snake {
    pub cells: VecDeque<(i32, i32)>,
    heading: Heading,
    next: Heading,
    pub food: (i32, i32),
    pub symbol: char,
    pub score: u32,
    pub over: bool,
    seed: u64,
    /// 最後に進めた時刻（画面の刻みで進める）
    last: Option<Instant>,
}

impl Snake {
    pub fn new(seed: u64) -> Self {
        let mut snake = Self {
            cells: VecDeque::from([(6, ROWS / 2), (5, ROWS / 2), (4, ROWS / 2)]),
            heading: Heading::Right,
            next: Heading::Right,
            food: (0, 0),
            symbol: '#',
            score: 0,
            over: false,
            seed: seed | 1,
            last: None,
        };
        snake.place_food();
        snake
    }

    fn random(&mut self) -> u64 {
        // xorshift（決まった種なら同じ並び。試験で使う）
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn place_food(&mut self) {
        loop {
            let x = (self.random() % COLUMNS as u64) as i32;
            let y = (self.random() % ROWS as u64) as i32;
            if !self.cells.contains(&(x, y)) {
                self.food = (x, y);
                self.symbol = SYMBOLS[(self.random() % SYMBOLS.len() as u64) as usize];
                return;
            }
        }
    }

    /// 向きを変える（真後ろへは向けない）
    pub fn turn(&mut self, heading: Heading) {
        if heading != self.heading.opposite() {
            self.next = heading;
        }
    }

    /// 1 歩進める
    pub fn step(&mut self) {
        if self.over {
            return;
        }
        self.heading = self.next;
        let (dx, dy) = self.heading.step();
        let (x, y) = self.cells[0];
        let head = (x + dx, y + dy);
        let outside = head.0 < 0 || head.0 >= COLUMNS || head.1 < 0 || head.1 >= ROWS;
        // 尻尾はこの歩で動くので、尻尾の所へは入れる
        let body = self
            .cells
            .iter()
            .take(self.cells.len() - 1)
            .any(|cell| *cell == head);
        if outside || body {
            self.over = true;
            return;
        }
        self.cells.push_front(head);
        if head == self.food {
            self.score += 1;
            self.place_food();
        } else {
            self.cells.pop_back();
        }
    }

    /// 1 歩の間（集めるほど速くなる）
    fn interval(&self) -> Duration {
        Duration::from_millis(150u64.saturating_sub(u64::from(self.score) * 4).max(60))
    }
}

/// 作った記録の行（CHANGELOG の版の見出しと、その要約）
fn roll_lines() -> Vec<(String, bool)> {
    const CHANGELOG: &str = include_str!("../../CHANGELOG.md");
    let mut lines = vec![("mdview の歩み".to_owned(), true), (String::new(), false)];
    let mut current: Option<String> = None;
    let mut summary = String::new();
    let flush =
        |current: &mut Option<String>, summary: &mut String, lines: &mut Vec<(String, bool)>| {
            if let Some(version) = current.take() {
                lines.push((version, true));
                for chunk in chunks(summary.trim(), 34) {
                    lines.push((chunk, false));
                }
                lines.push((String::new(), false));
            }
            summary.clear();
        };
    let mut in_summary = false;
    for line in CHANGELOG.lines() {
        if let Some(rest) = line.strip_prefix("## [") {
            flush(&mut current, &mut summary, &mut lines);
            if rest.starts_with("Unreleased") {
                continue;
            }
            let (version, date) = rest
                .split_once("] - ")
                .unwrap_or((rest.trim_end_matches(']'), ""));
            current = Some(if date.is_empty() {
                format!("v{version}")
            } else {
                format!("v{version}（{date}）")
            });
            in_summary = true;
            continue;
        }
        if current.is_some() && in_summary {
            if line.starts_with("###") {
                in_summary = false;
            } else if !line.trim().is_empty() {
                summary.push_str(line.trim());
            }
        }
    }
    flush(&mut current, &mut summary, &mut lines);
    lines.push((String::new(), false));
    lines.push((
        "おしまい — 使ってくださって、ありがとうございます。".to_owned(),
        true,
    ));
    lines
}

/// 字の数で区切る（折り返しの代わり）。**区切りは句読点・中黒・空白・閉じかっこの後ろを優先する**（語の途中で切らない）
fn chunks(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + width).min(chars.len());
        if end < chars.len() {
            let back = (start + width / 2..end)
                .rev()
                .find(|&at| matches!(chars[at - 1], '、' | '。' | '・' | ' ' | '）' | '」' | '/'));
            if let Some(at) = back {
                end = at;
            }
        }
        out.push(
            chars[start..end]
                .iter()
                .collect::<String>()
                .trim()
                .to_owned(),
        );
        start = end;
    }
    out
}

impl App {
    /// About の決まった所を押した
    pub(super) fn egg_poke(&mut self, spot: EggSpot) {
        let state = &mut self.egg;
        match spot {
            EggSpot::Title => {
                state.title += 1;
                if state.title >= 5 {
                    state.title = 0;
                    let day = chrono::Local::now()
                        .format("%j")
                        .to_string()
                        .parse::<usize>()
                        .unwrap_or(0);
                    state.words += 1;
                    state.egg = Some(Egg::Word((day + state.words) % WORDS.len()));
                }
            }
            EggSpot::Version => {
                state.version += 1;
                if state.version >= 3 {
                    state.version = 0;
                    state.egg = Some(Egg::Picture(Instant::now()));
                }
            }
            EggSpot::Corner => {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(7, |since| since.as_nanos() as u64);
                state.egg = Some(Egg::Game(Snake::new(seed)));
            }
        }
    }

    /// 打鍵（作った記録の合言葉・ゲームの向き）
    pub(super) fn egg_key(&mut self, key: EggKey) {
        if let Some(Egg::Game(snake)) = self.egg.egg.as_mut() {
            match key {
                EggKey::Up => snake.turn(Heading::Up),
                EggKey::Down => snake.turn(Heading::Down),
                EggKey::Left => snake.turn(Heading::Left),
                EggKey::Right => snake.turn(Heading::Right),
                EggKey::Enter if snake.over => {
                    *snake = Snake::new(u64::from(snake.score) * 31 + 17)
                }
                _ => {}
            }
            return;
        }
        const CODE: [EggKey; 10] = [
            EggKey::Up,
            EggKey::Up,
            EggKey::Down,
            EggKey::Down,
            EggKey::Left,
            EggKey::Right,
            EggKey::Left,
            EggKey::Right,
            EggKey::B,
            EggKey::A,
        ];
        let state = &mut self.egg;
        state.konami = if CODE[state.konami] == key {
            state.konami + 1
        } else if CODE[0] == key {
            1
        } else {
            0
        };
        if state.konami == CODE.len() {
            state.konami = 0;
            state.egg = Some(Egg::Roll(Instant::now()));
        }
    }

    /// 画面の刻み（ゲームを進める）
    pub(super) fn egg_tick(&mut self) {
        if let Some(Egg::Game(snake)) = self.egg.egg.as_mut() {
            let now = Instant::now();
            let due = snake
                .last
                .is_none_or(|last| now.duration_since(last) >= snake.interval());
            if due {
                snake.last = Some(now);
                snake.step();
            }
        }
    }

    /// 動いているか（画面の刻みを求めるか）
    pub(super) fn egg_animating(&self) -> bool {
        matches!(
            self.egg.egg,
            Some(Egg::Picture(_) | Egg::Game(_) | Egg::Roll(_))
        )
    }

    /// About を閉じたとき（押した回数も忘れる）
    pub(super) fn egg_reset(&mut self) {
        self.egg = EggState::default();
    }

    /// ひとこと（出していなければ `None`）
    pub(super) fn egg_word(&self) -> Option<&'static str> {
        match self.egg.egg {
            Some(Egg::Word(index)) => Some(WORDS[index % WORDS.len()]),
            _ => None,
        }
    }

    /// 絵・ゲーム・作った記録の場所（出していなければ `None`）
    pub(super) fn egg_view(&self) -> Option<Element<'_, Message>> {
        let egg = self.egg.egg.as_ref()?;
        if matches!(egg, Egg::Word(_)) {
            return None;
        }
        let palette = self.theme().palette();
        Some(
            EggView {
                egg,
                text: palette.text,
                accent: palette.primary,
                icon: icon_handle(),
            }
            .into(),
        )
    }

    /// 試験の口で見せる名前
    pub(super) fn egg_name(&self) -> Option<&'static str> {
        Some(match self.egg.egg.as_ref()? {
            Egg::Word(_) => "word",
            Egg::Picture(_) => "picture",
            Egg::Game(_) => "game",
            Egg::Roll(_) => "roll",
        })
    }

    /// ゲームの点（試験の口）
    pub(super) fn egg_score(&self) -> Option<(u32, bool)> {
        match self.egg.egg.as_ref()? {
            Egg::Game(snake) => Some((snake.score, snake.over)),
            _ => None,
        }
    }
}

/// アプリのアイコン（`assets/icon.svg` を 1 度だけ描く）
fn icon_handle() -> image::Handle {
    static ICON: std::sync::OnceLock<image::Handle> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        match crate::embed::diagram::rasterize_scaled(
            include_str!("../../assets/icon.svg"),
            1.0,
            128,
        ) {
            Ok(picture) => image::Handle::from_rgba(
                picture.width,
                picture.height,
                picture.pixels.as_ref().clone(),
            ),
            Err(_) => image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
        }
    })
    .clone()
}

/// 絵・ゲーム・作った記録を描く所
struct EggView<'a> {
    egg: &'a Egg,
    text: Color,
    accent: Color,
    icon: image::Handle,
}

/// 描く所の大きさ
const AREA: Size = Size::new(572.0, 300.0);

fn label<Renderer>(
    renderer: &mut Renderer,
    content: &str,
    at: Point,
    size: f32,
    color: Color,
    clip: Rectangle,
    center: bool,
) where
    Renderer: text::Renderer<Font = iced::Font>,
{
    renderer.fill_text(
        Text {
            content: content.to_owned(),
            bounds: Size::new(AREA.width, size * 1.6),
            size: Pixels(size),
            line_height: text::LineHeight::Absolute(Pixels(size * 1.6)),
            font: crate::render::fonts::for_style(crate::layout::TextStyle::Body, size).0,
            align_x: if center {
                text::Alignment::Center
            } else {
                text::Alignment::Left
            },
            align_y: iced::alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        },
        at,
        color,
        clip,
    );
}

impl<Theme, Renderer> Widget<Message, Theme, Renderer> for EggView<'_>
where
    Renderer: text::Renderer<Font = iced::Font> + image::Renderer<Handle = image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(AREA.width), Length::Fixed(AREA.height))
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(AREA)
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: Color {
                        a: 0.2,
                        ..self.text
                    },
                },
                ..Default::default()
            },
            Color {
                a: 0.05,
                ..self.text
            },
        );
        renderer.with_layer(bounds, |renderer| match self.egg {
            Egg::Picture(started) => {
                let t = started.elapsed().as_secs_f32();
                // 降る記号（決まった並び。字ごとに速さと位置をずらす）
                for index in 0..42u32 {
                    let hash = index.wrapping_mul(2_654_435_761);
                    let x = (hash % 1000) as f32 / 1000.0 * (bounds.width - 20.0);
                    let speed = 30.0 + (hash >> 10) as f32 % 70.0;
                    let phase = (hash >> 4) as f32 % bounds.height;
                    let y = (t * speed + phase) % (bounds.height + 30.0) - 30.0;
                    let symbol = SYMBOLS[(index as usize) % SYMBOLS.len()].to_string();
                    let color = if index % 3 == 0 {
                        self.accent
                    } else {
                        Color {
                            a: 0.45,
                            ..self.text
                        }
                    };
                    label(
                        renderer,
                        &symbol,
                        Point::new(bounds.x + x, bounds.y + y),
                        16.0,
                        color,
                        bounds,
                        false,
                    );
                }
                // 跳ねるアイコン
                let size = 72.0;
                let x =
                    bounds.center_x() + (t * 1.3).sin() * (bounds.width / 2.0 - size) - size / 2.0;
                let y = bounds.y + bounds.height - size - (t * 3.2).sin().abs() * 150.0 - 12.0;
                renderer.draw_image(
                    image::Image::new(self.icon.clone()),
                    Rectangle::new(Point::new(x, y), Size::new(size, size)),
                    bounds,
                );
            }
            Egg::Game(snake) => {
                let cell =
                    (bounds.width / COLUMNS as f32).min((bounds.height - 28.0) / ROWS as f32);
                let left = bounds.x + (bounds.width - cell * COLUMNS as f32) / 2.0;
                let top = bounds.y + 24.0;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(left, top),
                            Size::new(cell * COLUMNS as f32, cell * ROWS as f32),
                        ),
                        ..Default::default()
                    },
                    Color {
                        a: 0.06,
                        ..self.text
                    },
                );
                for (number, (x, y)) in snake.cells.iter().enumerate() {
                    let color = if number == 0 {
                        self.accent
                    } else {
                        Color {
                            a: 0.75,
                            ..self.accent
                        }
                    };
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(
                                    left + *x as f32 * cell + 1.0,
                                    top + *y as f32 * cell + 1.0,
                                ),
                                Size::new(cell - 2.0, cell - 2.0),
                            ),
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        color,
                    );
                }
                let (fx, fy) = snake.food;
                label(
                    renderer,
                    &snake.symbol.to_string(),
                    Point::new(
                        left + fx as f32 * cell + cell * 0.25,
                        top + fy as f32 * cell - cell * 0.15,
                    ),
                    cell * 0.9,
                    self.text,
                    bounds,
                    false,
                );
                let line = if snake.over {
                    format!(
                        "おしまい: 記号を {} 個。Enter でもう一度・Esc で戻る",
                        snake.score
                    )
                } else {
                    format!("集めた記号: {}　（矢印で動かす・Esc で戻る）", snake.score)
                };
                label(
                    renderer,
                    &line,
                    Point::new(bounds.x + 10.0, bounds.y + 2.0),
                    12.0,
                    self.text,
                    bounds,
                    false,
                );
            }
            Egg::Roll(started) => {
                let lines = roll_lines();
                let t = started.elapsed().as_secs_f32();
                let step = 24.0;
                let total = lines.len() as f32 * step + bounds.height;
                let offset = (t * 32.0) % total;
                for (number, (content, strong)) in lines.iter().enumerate() {
                    let y = bounds.y + bounds.height - offset + number as f32 * step;
                    if y < bounds.y - step || y > bounds.y + bounds.height {
                        continue;
                    }
                    let (size, color) = if *strong {
                        (15.0, self.accent)
                    } else {
                        (12.0, self.text)
                    };
                    label(
                        renderer,
                        content,
                        Point::new(bounds.center_x(), y),
                        size,
                        color,
                        bounds,
                        true,
                    );
                }
            }
            Egg::Word(_) => {}
        });
    }
}

impl<'a, Theme, Renderer> From<EggView<'a>> for Element<'a, Message, Theme, Renderer>
where
    Theme: 'a,
    Renderer: text::Renderer<Font = iced::Font> + image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(view: EggView<'a>) -> Self {
        Element::new(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snake_eats_and_grows() {
        let mut snake = Snake::new(42);
        // 食べ物をヘビの真ん前に置く
        snake.food = (7, ROWS / 2);
        snake.step();
        assert_eq!(snake.score, 1);
        assert_eq!(snake.cells.len(), 4);
        assert_ne!(snake.food, (7, ROWS / 2), "次の食べ物を置いた");
    }

    #[test]
    fn walls_and_the_body_end_the_game() {
        let mut snake = Snake::new(1);
        snake.food = (0, 0);
        for _ in 0..COLUMNS {
            snake.step();
        }
        assert!(snake.over, "右の壁にぶつかる");
        // 真後ろへは向けない
        let mut snake = Snake::new(3);
        snake.turn(Heading::Left);
        snake.food = (0, 0);
        snake.step();
        assert!(!snake.over);
        assert_eq!(snake.cells[0], (7, ROWS / 2));
    }

    #[test]
    fn chunks_break_after_punctuation() {
        let parts = chunks(
            "背景画像・コマンドパレット・プレビューから行へ・図の拡大と PNG・コードブロック",
            20,
        );
        assert!(
            parts.iter().all(|part| part.chars().count() <= 20),
            "{parts:?}"
        );
        assert!(parts[0].ends_with('・'), "{parts:?}");
    }

    #[test]
    fn the_roll_lists_versions() {
        let lines = roll_lines();
        assert!(lines
            .iter()
            .any(|(line, strong)| *strong && line.starts_with("v2.4.0")));
        assert!(!lines.iter().any(|(line, _)| line.contains("Unreleased")));
    }
}
