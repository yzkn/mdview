//! 背景画像を読み込んで敷く（v2.4.0 R-01）。
//!
//! 設定と画像の下ごしらえ（縮める・ぼかす）は `crate::io::backdrop`、描く部品は
//! `crate::render::backdrop`。ここでは**いまの配色で要る画像を裏の糸で読み**、読めたら
//! 描くための `Handle` を 1 度だけ作って持つ（描くたびに作らない。大きい画像を毎回送らない）。
//!
//! 配色が明るい・暗いで変わったとき（日の出・日の入りを含む）・設定を変えたときは、
//! `update` の出口（`refresh_backdrop`）で要る画像を見直す。

use super::fluent;
use iced::advanced::image;
use iced::widget::{button, column, container, row, stack, text};
use iced::{Element, Length, Task};

use super::settings_view::SettingChange;
use super::App;
use crate::io::backdrop::{self, Area, Wanted};
use crate::render::Message;

/// 読み込みの様子
#[derive(Debug, Default)]
pub struct BackdropImage {
    /// 読んでいる・読んだ画像（同じなら読み直さない）
    pub wanted: Option<Wanted>,
    /// 描くもの（読めたとき）
    pub drawn: Option<(image::Handle, (u32, u32))>,
    /// 読めなかった理由（設定画面に出す）
    pub error: Option<String>,
}

/// どこに敷く層か
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layer {
    Window,
    Editor,
    Preview,
    /// 分割表示の 2 つにまたがる
    Span,
}

/// 透明度の選択肢（%）
const OPACITY_STEPS: [u8; 21] = [
    0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100,
];

/// 選べる画像の拡張子（`image` で読めるもの）
const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "bmp", "webp", "jfif"];

impl App {
    /// いまの配色が暗いか
    fn backdrop_dark(&self) -> bool {
        self.theme().extended_palette().is_dark
    }

    /// 要る画像を見直し、変わっていれば裏の糸で読む（`update` の出口で呼ぶ）
    pub(super) fn refresh_backdrop(&mut self) -> Task<Message> {
        let wanted = self.settings.backdrop.wanted(self.backdrop_dark());
        if wanted == self.backdrop.wanted {
            return Task::none();
        }
        self.backdrop.wanted.clone_from(&wanted);
        self.backdrop.drawn = None;
        self.backdrop.error = None;
        let Some(wanted) = wanted else {
            return Task::none();
        };
        // 試験ではディスクの画像を読まない（設定の往復だけを見る）
        if cfg!(test) {
            return Task::none();
        }
        Task::perform(
            async move {
                let loaded = backdrop::load(&wanted).map(std::sync::Arc::new);
                (wanted, loaded)
            },
            |(wanted, loaded)| Message::BackdropLoaded(wanted, loaded),
        )
    }

    /// 読み込みの結果。**頼んだときと鍵が違えば捨てる**（読んでいる間に設定が変わった）
    pub(super) fn backdrop_loaded(
        &mut self,
        wanted: Wanted,
        loaded: Result<std::sync::Arc<backdrop::Loaded>, String>,
    ) {
        if self.backdrop.wanted.as_ref() != Some(&wanted) {
            return;
        }
        match loaded {
            Ok(image) => {
                let handle =
                    image::Handle::from_rgba(image.width, image.height, image.rgba.clone());
                self.backdrop.drawn = Some((handle, (image.width, image.height)));
                self.backdrop.error = None;
            }
            Err(reason) => {
                self.backdrop.drawn = None;
                self.backdrop.error = Some(reason);
            }
        }
    }

    /// この層に敷くか（設定の「表示範囲」と、いまの表示モード）
    fn backdrop_on(&self, layer: Layer) -> bool {
        let split = self.mode == crate::render::ViewMode::Split;
        match (self.settings.backdrop.area, layer) {
            (Area::Window, Layer::Window) => true,
            (Area::Panes, Layer::Editor | Layer::Preview) => true,
            (Area::Editor, Layer::Editor) => true,
            (Area::Preview, Layer::Preview) => true,
            // **またがる 1 枚は分割表示のときだけ。** 1 つの領域しか出ていなければ、その領域に敷く
            (Area::Span, Layer::Span) => split,
            (Area::Span, Layer::Editor | Layer::Preview) => !split,
            _ => false,
        }
    }

    /// 本文と一緒に動かすときの送り量（px）。プレビューはブロックの番号からの目安
    fn backdrop_offset(&self, layer: Layer) -> f32 {
        if !self.settings.backdrop.scroll {
            return 0.0;
        }
        match layer {
            Layer::Editor | Layer::Span => self.editor.top_line as f32 * self.editor_sizes().1,
            Layer::Preview => {
                let anchor = &self.preview.anchor;
                anchor.block_id as f32 * PREVIEW_BLOCK_GUESS + anchor.offset_in_block
            }
            Layer::Window => 0.0,
        }
    }

    /// 試験の口が見る送り量（編集画面とプレビュー。本文と一緒に動かすときだけ 0 でない）
    pub(super) fn backdrop_offset_for_automation(&self) -> [f32; 2] {
        [
            self.backdrop_offset(Layer::Editor),
            self.backdrop_offset(Layer::Preview),
        ]
    }

    /// `content` の下に背景画像を敷く（敷かないなら `content` のまま）
    pub(super) fn with_backdrop<'a>(
        &self,
        layer: Layer,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let content = content.into();
        let drawn = self
            .backdrop
            .drawn
            .as_ref()
            .filter(|_| self.settings.backdrop.enabled && self.backdrop_on(layer));
        let Some((handle, size)) = drawn else {
            // Mica 風（v2.4.0 R-05）で「本文の地を残す」なら、編集画面とプレビューに地を重ねる
            if matches!(layer, Layer::Editor | Layer::Preview) {
                if let Some(tint) = self.mica_body_tint() {
                    return stack![super::mica_view::fill(tint), content].into();
                }
            }
            return content;
        };
        let look = self.settings.backdrop.look(self.backdrop_dark());
        let backdrop = &self.settings.backdrop;
        let layer_element: Element<'a, Message> = crate::render::backdrop::Backdrop::new(
            handle.clone(),
            *size,
            backdrop.fit,
            backdrop.anchor,
        )
        .opacity(f32::from(look.opacity) / 100.0)
        .offset_y(self.backdrop_offset(layer))
        .into();
        // **背景画像を敷いた所は、Mica 風より背景画像を優先する**（下に不透明な地を敷く。v2.4.0 R-05）
        match self.mica_opaque() {
            Some(color) => stack![super::mica_view::fill(color), layer_element, content].into(),
            None => stack![layer_element, content].into(),
        }
    }

    /// 設定画面「外観」の背景画像の欄（`crate::io::backdrop::KEYS` と同じ並び）
    pub(super) fn backdrop_section(&self) -> Element<'_, Message> {
        let s = &self.settings.backdrop;
        let page = column![
            super::settings_view::heading("背景画像"),
            self.backdrop_item(
                "背景画像",
                "bg_enabled",
                fluent::checkbox(s.enabled)
                    .label("使う")
                    .text_size(12)
                    .on_toggle(|on| set("bg_enabled", on.to_string()))
                    .into(),
            ),
            self.backdrop_choice("表示範囲", "bg_area", &Area::ALL, s.area, |a| a.key()),
            self.backdrop_choice("表示方法", "bg_fit", &backdrop::Fit::ALL, s.fit, |f| f.key()),
            self.backdrop_choice(
                "配置",
                "bg_anchor",
                &backdrop::Anchor::ALL,
                s.anchor,
                |a| a.key()
            ),
            self.backdrop_item(
                "動き方",
                "bg_scroll",
                fluent::checkbox(s.scroll)
                    .label("本文と一緒に動かす（切ると窓に固定）")
                    .text_size(12)
                    .on_toggle(|on| set("bg_scroll", on.to_string()))
                    .into(),
            ),
            self.backdrop_image_line("明るい配色の画像", "bg_light_image", &s.light.image, false),
            self.backdrop_opacity("明るい配色の濃さ（%）", "bg_light_opacity", s.light.opacity),
            self.backdrop_choice(
                "明るい配色のぼかし",
                "bg_light_blur",
                &backdrop::Blur::ALL,
                s.light.blur,
                |b| b.key()
            ),
            self.backdrop_image_line("暗い配色の画像", "bg_dark_image", &s.dark.image, true),
            self.backdrop_opacity("暗い配色の濃さ（%）", "bg_dark_opacity", s.dark.opacity),
            self.backdrop_choice(
                "暗い配色のぼかし",
                "bg_dark_blur",
                &backdrop::Blur::ALL,
                s.dark.blur,
                |b| b.key()
            ),
            super::settings_view::note(
                "暗い配色の画像が空なら、明るい配色の画像を使います。濃さは小さいほど薄く、文字が読みやすくなります。PDF・HTML には入りません。"
            ),
        ]
        .spacing(6);
        let mut page = page;
        if let Some(error) = &self.backdrop.error {
            page = page.push(super::settings_view::note(error));
        }
        // **見本**（いま読めている画像を小さく出す）
        if let Some((handle, size)) = &self.backdrop.drawn {
            let look = s.look(self.backdrop_dark());
            let sample: Element<'_, Message> =
                crate::render::backdrop::Backdrop::new(handle.clone(), *size, s.fit, s.anchor)
                    .opacity(f32::from(look.opacity) / 100.0)
                    .into();
            page = page.push(super::settings_view::line(
                "見本",
                container(sample)
                    .width(Length::Fixed(240.0))
                    .height(Length::Fixed(135.0))
                    .style(container::bordered_box),
            ));
        }
        page.into()
    }

    /// 1 項目（右に「既定」）
    fn backdrop_item<'a>(
        &self,
        label: &'a str,
        key: &str,
        control: Element<'a, Message>,
    ) -> Element<'a, Message> {
        super::settings_view::with_reset(
            super::settings_view::line(label, control),
            super::settings_view::reset_change(key, &self.settings),
        )
    }

    /// 選ぶ項目。**画面には日本語の名前、設定には鍵**
    fn backdrop_choice<'a, T>(
        &self,
        label: &'a str,
        key: &'static str,
        all: &[T],
        now: T,
        to_key: fn(T) -> &'static str,
    ) -> Element<'a, Message>
    where
        T: std::fmt::Display + Copy + PartialEq + 'a,
    {
        self.backdrop_item(
            label,
            key,
            fluent::pick_list(all.to_vec(), Some(now), move |choice| {
                set(key, to_key(choice).to_owned())
            })
            .text_size(12)
            .into(),
        )
    }

    fn backdrop_opacity<'a>(
        &self,
        label: &'a str,
        key: &'static str,
        now: u8,
    ) -> Element<'a, Message> {
        self.backdrop_item(
            label,
            key,
            fluent::pick_list(OPACITY_STEPS.to_vec(), Some(now), move |value| {
                set(key, value.to_string())
            })
            .text_size(12)
            .into(),
        )
    }

    fn backdrop_image_line<'a>(
        &self,
        label: &'a str,
        key: &'static str,
        value: &'a str,
        dark: bool,
    ) -> Element<'a, Message> {
        self.backdrop_item(
            label,
            key,
            row![
                fluent::text_input("画像ファイル（PNG・JPEG・GIF・BMP・WebP）", value)
                    .on_input(move |value| set(key, value))
                    .size(12)
                    .width(Length::Fixed(360.0)),
                button(text("選ぶ…").size(11))
                    .padding([2, 10])
                    .on_press(Message::Setting(SettingChange::BackdropPick(dark))),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into(),
        )
    }

    /// 「選ぶ…」。**OS のダイアログ（出せなければアプリ内のもの）で画像を選ぶ**
    pub(super) fn pick_backdrop(&mut self, dark: bool) -> Task<Message> {
        if self.picking {
            return Task::none();
        }
        let picker = super::picker::Picker::open_kind("画像", &IMAGE_EXTENSIONS);
        self.ask(picker)
            .map(move |path| Message::Setting(SettingChange::BackdropPicked(dark, path)))
    }
}

/// プレビューの 1 ブロックの高さの目安（px。本文と一緒に動かすときの送り量に使う）
const PREVIEW_BLOCK_GUESS: f32 = 40.0;

/// 背景画像の 1 項目を変える
fn set(key: &str, value: String) -> Message {
    Message::Setting(SettingChange::Backdrop(key.to_owned(), value))
}
