//! 発表の画面（v2.4.0 R-07 7c）。Marp の文書を 1 枚ずつ全画面で見せる。
//!
//! 始めるのは「発表」（F5）。**キャレットのあるスライドから**始める。送るのは → ↓ PageDown Space Enter・
//! 押下・ホイール、戻すのは ← ↑ PageUp BackSpace・右の押下、Home と End で最初と最後、Esc で終わる。
//! 終わると、編集画面とプレビューを最後に見ていたスライドへ合わせる。
//! 設定で、右下にページ番号と経過時間を出せる（既定は出さない）。

use iced::{Element, Length, Task};

use super::App;
use crate::render::{Message, PreviewView};

/// 発表している最中の控え
#[derive(Debug, Clone)]
pub(super) struct Presenting {
    /// 何枚目を出しているか（0 から）
    pub slide: usize,
    pub started: std::time::Instant,
    /// 発表のために全画面にしたか（集中モードで全画面だったときは戻さない）
    entered_fullscreen: bool,
}

impl App {
    /// 発表を始める（している最中なら終える）
    pub(super) fn toggle_presentation(&mut self) -> Task<Message> {
        if self.presenting.is_some() {
            return self.end_presentation();
        }
        let Some(deck) = &self.deck else {
            self.notice = Some(super::notice::Notice::plain(
                "発表できるのは Marp の文書（frontmatter に marp: true）を、スライドに分けて見せているときです"
                    .to_owned(),
            ));
            return Task::none();
        };
        let block = self.document.block_at_byte(self.caret_byte()).unwrap_or(0);
        let slide = deck.slide_of(block).unwrap_or(0);
        let entered_fullscreen = !self.focus_mode;
        self.presenting = Some(Presenting {
            slide,
            started: std::time::Instant::now(),
            entered_fullscreen,
        });
        self.open_menu = None;
        self.completion = None;
        if entered_fullscreen {
            iced::window::latest().then(|id| match id {
                Some(id) => iced::window::set_mode(id, iced::window::Mode::Fullscreen),
                None => Task::none(),
            })
        } else {
            Task::none()
        }
    }

    /// 次・前へ送る（端では止まる）
    pub(super) fn step_presentation(&mut self, step: i32) {
        let count = self.deck.as_ref().map_or(0, |deck| deck.slides.len());
        if let Some(presenting) = self.presenting.as_mut() {
            let next = presenting.slide as i64 + i64::from(step);
            presenting.slide = next.clamp(0, count.saturating_sub(1) as i64) as usize;
        }
    }

    /// 何枚目かへ飛ぶ（`usize::MAX` なら最後）
    pub(super) fn present_to(&mut self, slide: usize) {
        let count = self.deck.as_ref().map_or(0, |deck| deck.slides.len());
        if let Some(presenting) = self.presenting.as_mut() {
            presenting.slide = slide.min(count.saturating_sub(1));
        }
    }

    /// 発表を終える。**編集画面とプレビューを、最後に見ていたスライドへ合わせる**
    pub(super) fn end_presentation(&mut self) -> Task<Message> {
        let Some(presenting) = self.presenting.take() else {
            return Task::none();
        };
        let place = self
            .deck
            .as_ref()
            .and_then(|deck| deck.slides.get(presenting.slide))
            .map(|slide| {
                let first = slide.content.first().copied().unwrap_or(slide.range.start);
                (first, slide.range.start)
            });
        if let Some((first, start)) = place {
            if let Some(block) = self.document.blocks().get(first) {
                let line = block.start_line;
                self.editor.place_caret(line, 0);
                self.editor.anchor = None;
                self.follow_cursor();
            }
            self.preview.anchor = crate::layout::ScrollAnchor {
                block_id: start,
                offset_in_block: 0.0,
            };
        }
        if presenting.entered_fullscreen {
            iced::window::latest().then(|id| match id {
                Some(id) => iced::window::set_mode(id, iced::window::Mode::Windowed),
                None => Task::none(),
            })
        } else {
            Task::none()
        }
    }

    /// 右下に出す文字（ページ番号と経過時間）。**設定で出すときだけ**
    fn present_info(&self, presenting: &Presenting) -> Option<String> {
        if !self.settings.marp_present_info {
            return None;
        }
        let count = self.deck.as_ref().map_or(0, |deck| deck.slides.len());
        let seconds = presenting.started.elapsed().as_secs();
        Some(format!(
            "{} / {}    {:02}:{:02}",
            presenting.slide + 1,
            count,
            seconds / 60,
            seconds % 60
        ))
    }

    /// 発表の画面（窓いっぱい）
    pub(super) fn present_view(&self) -> Option<Element<'_, Message>> {
        let presenting = self.presenting.as_ref()?;
        let deck = self.deck.as_ref()?;
        let view = PreviewView::new(&self.document, &self.preview, Message::Preview)
            .embeds(&self.embeds)
            .base_dir(self.meta.base_dir())
            .slides(Some(deck))
            .present(Some(presenting.slide), self.present_info(presenting));
        Some(
            iced::widget::container(view)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }
}

/// 発表の最中の打鍵から、送る・戻す（Esc は閉じる順の連なりで受ける）。**画面と試験の口で同じものを使う**
pub(super) fn message_for(key: super::keymap::NamedKey) -> Option<Message> {
    use super::keymap::NamedKey;
    match key {
        NamedKey::Right
        | NamedKey::Down
        | NamedKey::PageDown
        | NamedKey::Space
        | NamedKey::Enter => Some(Message::PresentStep(1)),
        NamedKey::Left | NamedKey::Up | NamedKey::PageUp | NamedKey::Backspace => {
            Some(Message::PresentStep(-1))
        }
        NamedKey::Home => Some(Message::PresentTo(0)),
        NamedKey::End => Some(Message::PresentTo(usize::MAX)),
        _ => None,
    }
}

/// 発表の最中の打鍵を受ける
pub(super) fn keys() -> iced::Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| {
        use super::keymap::NamedKey;
        use iced::keyboard::key::Named;
        let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { ref key, .. }) = event else {
            return None;
        };
        let iced::keyboard::Key::Named(named) = key.as_ref() else {
            return None;
        };
        let key = match named {
            Named::ArrowRight => NamedKey::Right,
            Named::ArrowDown => NamedKey::Down,
            Named::ArrowLeft => NamedKey::Left,
            Named::ArrowUp => NamedKey::Up,
            Named::PageDown => NamedKey::PageDown,
            Named::PageUp => NamedKey::PageUp,
            Named::Space => NamedKey::Space,
            Named::Enter => NamedKey::Enter,
            Named::Backspace => NamedKey::Backspace,
            Named::Home => NamedKey::Home,
            Named::End => NamedKey::End,
            _ => return None,
        };
        message_for(key)
    })
}
