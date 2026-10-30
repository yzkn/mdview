//! 3 つのファイルのマージの画面（v2.6.0 R-04 c）。共通の元・左・右から結果を作る。
//!
//! **左・結果・右の 3 面**（本文を覆う画面）。結果は共通の元から作り、片側だけの変更は取り込んである
//! （計算は `crate::diff::merge`）。両側で食い違う所（衝突）は印を付け、左を取る・右を取る・両方を取るで決める。
//!
//! 結果の面は比較の画面と同じく、**本文と面の文書を入れ替えて編集画面と同じ道で書き換える**
//! （[`App::in_doc`](super::App::in_doc)）。直したあとは、前後の行の違いからかたまりの行の範囲を直す
//! （衝突の中を手で直したら、手で決めたものとみなす）。左右の面は読むだけ。

use std::ops::Range;
use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, row, text};
use iced::{Color, Element, Length, Task};

use super::diff::PaneDoc;
use super::file::DocumentMeta;
use super::App;
use crate::diff::merge::{self, Region, RegionKind, Take};
use crate::document::{history::History, Document};
use crate::io::LoadedFile;
use crate::render::{Action, DiffMarks, EditorState, EditorView, LineMark, Message};

/// 衝突の色・取り込んだ所の色（比較の画面の色と同じ考え方で、半透明で重ねる）
const CONFLICT: Color = Color::from_rgba(0.90, 0.25, 0.25, 0.25);
const TAKEN: Color = Color::from_rgba(0.20, 0.70, 0.30, 0.20);

/// 3 つの面
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePane {
    Left,
    Result,
    Right,
}

/// マージの画面
pub struct MergeScreen {
    /// 共通の元（結果の保存先の既定の置き場）
    pub base_path: PathBuf,
    /// 取るときに使う左・右の原文
    pub left_text: String,
    pub right_text: String,
    pub left: PaneDoc,
    pub result: PaneDoc,
    pub right: PaneDoc,
    pub left_title: String,
    pub right_title: String,
    /// かたまり（結果の行の範囲は、結果を直すたびに直す）
    pub regions: Vec<Region>,
    /// 決めた衝突（取った・手で直した）
    pub resolved: Vec<bool>,
    /// 選んでいるかたまり
    pub current: Option<usize>,
    pub focus: MergePane,
    pub marks: [DiffMarks; 3],
    /// 衝突が残ったまま保存するかの確認・保存していないまま閉じるかの確認
    pub confirm_save: bool,
    pub confirm_close: bool,
}

impl MergeScreen {
    /// 残っている衝突の数
    pub fn remaining(&self) -> usize {
        self.regions
            .iter()
            .zip(&self.resolved)
            .filter(|(region, done)| region.kind == RegionKind::Conflict && !**done)
            .count()
    }

    /// 衝突の数
    pub fn conflicts(&self) -> usize {
        self.regions
            .iter()
            .filter(|region| region.kind == RegionKind::Conflict)
            .count()
    }

    fn doc(&self, pane: MergePane) -> &PaneDoc {
        match pane {
            MergePane::Left => &self.left,
            MergePane::Result => &self.result,
            MergePane::Right => &self.right,
        }
    }

    /// 面ごとの印を作り直す
    fn refresh_marks(&mut self) {
        let lines = |doc: &PaneDoc| doc.document.text().len_lines();
        let blank = |count: usize| DiffMarks {
            lines: vec![LineMark::Same; count],
            inline: std::collections::HashMap::new(),
            changed: CONFLICT,
            only: TAKEN,
            other: TAKEN,
            inline_color: TAKEN,
            spacer: Color::TRANSPARENT,
            current: None,
        };
        let mut marks = [
            blank(lines(&self.left)),
            blank(lines(&self.result)),
            blank(lines(&self.right)),
        ];
        let paint = |marks: &mut DiffMarks, range: &Range<usize>, mark: LineMark| {
            for line in range.clone() {
                if let Some(entry) = marks.lines.get_mut(line) {
                    *entry = mark;
                }
            }
        };
        for (index, region) in self.regions.iter().enumerate() {
            let open = region.kind == RegionKind::Conflict && !self.resolved[index];
            let result_mark = if open {
                LineMark::Changed
            } else {
                LineMark::Only
            };
            paint(&mut marks[1], &region.result, result_mark);
            let (left_mark, right_mark) = match region.kind {
                RegionKind::Conflict => (LineMark::Changed, LineMark::Changed),
                RegionKind::Left => (LineMark::Only, LineMark::Same),
                RegionKind::Right => (LineMark::Same, LineMark::Only),
                RegionKind::Both => (LineMark::Only, LineMark::Only),
            };
            paint(&mut marks[0], &region.left, left_mark);
            paint(&mut marks[2], &region.right, right_mark);
        }
        if let Some(region) = self.current.and_then(|n| self.regions.get(n)) {
            marks[0].current = Some(region.left.clone());
            marks[1].current = Some(region.result.clone());
            marks[2].current = Some(region.right.clone());
        }
        self.marks = marks;
    }

    /// 結果を `old` から `new` へ直したあと、かたまりの範囲を直す（行で比べる）
    fn shift_after_edit(&mut self, old: &str, new: &str) {
        let old_lines = crate::diff::lines(old);
        let new_lines = crate::diff::lines(new);
        let prefix = old_lines
            .iter()
            .zip(&new_lines)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = old_lines[prefix..]
            .iter()
            .rev()
            .zip(new_lines[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let old_end = old_lines.len() - suffix;
        let delta = (new_lines.len() - suffix) as isize - old_end as isize;
        for (index, region) in self.regions.iter_mut().enumerate() {
            let range = &mut region.result;
            if range.end <= prefix && !(range.start == range.end && range.start == prefix) {
                continue;
            }
            if range.start >= old_end {
                range.start = (range.start as isize + delta) as usize;
                range.end = (range.end as isize + delta) as usize;
                continue;
            }
            // 直した所に掛かる。**手で決めたものとみなす**
            let start = range.start.min(prefix);
            let end = (range.end.max(old_end) as isize + delta).max(start as isize) as usize;
            *range = start..end;
            self.resolved[index] = true;
        }
    }
}

/// マージの画面へのメッセージ
#[derive(Debug, Clone)]
pub enum MergeMessage {
    /// 「3 つのファイルをマージする」
    Open,
    PickedBase(Option<PathBuf>),
    PickedLeft {
        base: PathBuf,
        path: Option<PathBuf>,
    },
    PickedRight {
        base: PathBuf,
        left: PathBuf,
        path: Option<PathBuf>,
    },
    #[allow(clippy::type_complexity)]
    Loaded(Result<(Box<LoadedFile>, Box<LoadedFile>, Box<LoadedFile>), String>),
    /// 面の編集部品からの通知
    Pane(MergePane, Action),
    /// 選んでいる衝突を決める
    Take(Take),
    Next,
    Previous,
    Save,
    SaveTo(Option<PathBuf>),
    /// 衝突が残ったまま保存するか（`true` なら保存先を聞く）
    ConfirmSave(bool),
    Close,
    ConfirmClose(bool),
}

fn pane_doc(file: LoadedFile) -> PaneDoc {
    let meta = DocumentMeta::opened(file.path, file.format);
    PaneDoc {
        document: Document::from_text(file.text),
        editor: EditorState::default(),
        history: History::new(),
        meta,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl App {
    /// マージの画面へのメッセージを処理する
    pub(super) fn merge_message(&mut self, message: MergeMessage) -> Task<Message> {
        match message {
            MergeMessage::Open => {
                if self.picking || self.merge_screen.is_some() || self.diff_screen.is_some() {
                    return Task::none();
                }
                self.notice = Some(super::notice::Notice::plain(
                    "マージする 1 つ目：共通の元（左右が分かれる前）のファイルを選んでください"
                        .to_owned(),
                ));
                return self
                    .ask(super::picker::Picker::open())
                    .map(|path| Message::Merge(MergeMessage::PickedBase(path)));
            }
            MergeMessage::PickedBase(path) => {
                self.picking = false;
                self.browser_choice = None;
                let Some(base) = path else {
                    self.notice = None;
                    return Task::none();
                };
                self.notice = Some(super::notice::Notice::plain(
                    "マージする 2 つ目：左のファイルを選んでください".to_owned(),
                ));
                return self.ask(super::picker::Picker::open()).map(move |path| {
                    Message::Merge(MergeMessage::PickedLeft {
                        base: base.clone(),
                        path,
                    })
                });
            }
            MergeMessage::PickedLeft { base, path } => {
                self.picking = false;
                self.browser_choice = None;
                let Some(left) = path else {
                    self.notice = None;
                    return Task::none();
                };
                self.notice = Some(super::notice::Notice::plain(
                    "マージする 3 つ目：右のファイルを選んでください".to_owned(),
                ));
                return self.ask(super::picker::Picker::open()).map(move |path| {
                    Message::Merge(MergeMessage::PickedRight {
                        base: base.clone(),
                        left: left.clone(),
                        path,
                    })
                });
            }
            MergeMessage::PickedRight { base, left, path } => {
                self.picking = false;
                self.browser_choice = None;
                let Some(right) = path else {
                    self.notice = None;
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        let load = |path: &Path| {
                            crate::io::load_as(path, None)
                                .map(Box::new)
                                .map_err(|error| format!("{}: {error}", path.display()))
                        };
                        Ok((load(&base)?, load(&left)?, load(&right)?))
                    },
                    |result| Message::Merge(MergeMessage::Loaded(result)),
                );
            }
            MergeMessage::Loaded(result) => match result {
                Ok((base, left, right)) => self.open_merge(*base, *left, *right),
                Err(error) => self.notice = Some(super::notice::Notice::plain(error)),
            },
            MergeMessage::Pane(pane, action) => return self.merge_pane_action(pane, action),
            MergeMessage::Take(choice) => self.merge_take(choice),
            MergeMessage::Next => self.merge_step(true),
            MergeMessage::Previous => self.merge_step(false),
            MergeMessage::Save => return self.merge_save(false),
            MergeMessage::ConfirmSave(go) => {
                if let Some(screen) = self.merge_screen.as_mut() {
                    screen.confirm_save = false;
                }
                if go {
                    return self.merge_save(true);
                }
            }
            MergeMessage::SaveTo(path) => {
                self.picking = false;
                self.browser_choice = None;
                if let Some(path) = path {
                    self.merge_write(&path);
                }
            }
            MergeMessage::Close => {
                let Some(screen) = self.merge_screen.as_mut() else {
                    return Task::none();
                };
                if screen.result.meta.dirty && !screen.confirm_close {
                    screen.confirm_close = true;
                    return Task::none();
                }
                self.merge_screen = None;
                self.notice = None;
            }
            MergeMessage::ConfirmClose(close) => {
                if close {
                    self.merge_screen = None;
                    self.notice = None;
                } else if let Some(screen) = self.merge_screen.as_mut() {
                    screen.confirm_close = false;
                }
            }
        }
        Task::none()
    }

    /// 3 つを読み終えたので画面を作る
    pub(super) fn open_merge(&mut self, base: LoadedFile, left: LoadedFile, right: LoadedFile) {
        let merged = merge::merge(
            &base.text,
            &left.text,
            &right.text,
            self.settings.merge_markers,
        );
        let mut result_meta = DocumentMeta::opened(base.path.clone(), base.format.clone());
        // 結果はまだどこにも保存していない
        result_meta.dirty = true;
        let resolved = vec![false; merged.regions.len()];
        let (left_title, right_title) = (
            format!("左: {}", file_name(&left.path)),
            format!("右: {}", file_name(&right.path)),
        );
        let mut screen = MergeScreen {
            base_path: base.path.clone(),
            left_text: left.text.clone(),
            right_text: right.text.clone(),
            left: pane_doc(left),
            result: PaneDoc {
                document: Document::from_text(merged.text),
                editor: EditorState::default(),
                history: History::new(),
                meta: result_meta,
            },
            right: pane_doc(right),
            left_title,
            right_title,
            regions: merged.regions,
            resolved,
            current: None,
            focus: MergePane::Result,
            marks: Default::default(),
            confirm_save: false,
            confirm_close: false,
        };
        screen.refresh_marks();
        let conflicts = screen.conflicts();
        self.notice = Some(super::notice::Notice::plain(if conflicts == 0 {
            "衝突はありません。片側だけの変更を取り込みました（結果を保存してください）".to_owned()
        } else {
            format!("衝突が {conflicts} か所あります。左・右・両方のどれを取るか決めてください")
        }));
        self.merge_screen = Some(screen);
        if conflicts > 0 {
            self.merge_step(true);
        }
    }

    /// 面の編集部品からの通知。結果の面は書き換え、左右の面は読むだけ
    fn merge_pane_action(&mut self, pane: MergePane, action: Action) -> Task<Message> {
        let Some(mut screen) = self.merge_screen.take() else {
            return Task::none();
        };
        screen.focus = pane;
        let task = match pane {
            MergePane::Left => self.in_doc(&mut screen.left, true, |app| app.apply(action)),
            MergePane::Right => self.in_doc(&mut screen.right, true, |app| app.apply(action)),
            MergePane::Result => {
                let before = screen.result.document.text().to_string();
                let task = self.in_doc(&mut screen.result, false, |app| app.apply(action));
                let after = screen.result.document.text().to_string();
                if before != after {
                    screen.shift_after_edit(&before, &after);
                    screen.refresh_marks();
                }
                task
            }
        };
        self.merge_screen = Some(screen);
        task
    }

    /// 打鍵を受けている面へメッセージを回す（`update` の入口から。編集の操作）
    /// マージの面で `f` を行う（検索。v2.8.0 R-02）。左右は読むだけ
    pub(super) fn in_merge_pane<R>(&mut self, pane: MergePane, f: impl FnOnce(&mut App) -> R) -> R {
        let Some(mut screen) = self.merge_screen.take() else {
            return f(self);
        };
        let result = match pane {
            MergePane::Left => self.in_doc(&mut screen.left, true, f),
            MergePane::Right => self.in_doc(&mut screen.right, true, f),
            MergePane::Result => self.in_doc(&mut screen.result, false, f),
        };
        self.merge_screen = Some(screen);
        result
    }

    pub(super) fn route_to_merge(&mut self, message: Message) -> Task<Message> {
        let Some(mut screen) = self.merge_screen.take() else {
            return Task::none();
        };
        let task = match screen.focus {
            MergePane::Left => self.in_doc(&mut screen.left, true, |app| app.handle(message)),
            MergePane::Right => self.in_doc(&mut screen.right, true, |app| app.handle(message)),
            MergePane::Result => {
                let before = screen.result.document.text().to_string();
                let task = self.in_doc(&mut screen.result, false, |app| app.handle(message));
                let after = screen.result.document.text().to_string();
                if before != after {
                    screen.shift_after_edit(&before, &after);
                    screen.refresh_marks();
                }
                task
            }
        };
        self.merge_screen = Some(screen);
        task
    }

    /// 次・前の衝突へ（残っているものを優先。無ければかたまりを順に）
    fn merge_step(&mut self, forward: bool) {
        let Some(screen) = self.merge_screen.as_ref() else {
            return;
        };
        let open: Vec<usize> = (0..screen.regions.len())
            .filter(|&n| screen.regions[n].kind == RegionKind::Conflict && !screen.resolved[n])
            .collect();
        let candidates: Vec<usize> = if open.is_empty() {
            (0..screen.regions.len()).collect()
        } else {
            open
        };
        if candidates.is_empty() {
            return;
        }
        let next = match (screen.current, forward) {
            (Some(now), true) => candidates
                .iter()
                .copied()
                .find(|n| *n > now)
                .unwrap_or(candidates[0]),
            (Some(now), false) => candidates
                .iter()
                .rev()
                .copied()
                .find(|n| *n < now)
                .unwrap_or(candidates[candidates.len() - 1]),
            (None, true) => candidates[0],
            (None, false) => candidates[candidates.len() - 1],
        };
        self.merge_goto(next);
    }

    /// かたまりへ移る（3 面とも、その頭を上に 3 行の余白を残して見せる）
    fn merge_goto(&mut self, n: usize) {
        let Some(screen) = self.merge_screen.as_mut() else {
            return;
        };
        let Some(region) = screen.regions.get(n).cloned() else {
            return;
        };
        screen.current = Some(n);
        for (doc, line) in [
            (&mut screen.left, region.left.start),
            (&mut screen.result, region.result.start),
            (&mut screen.right, region.right.start),
        ] {
            let last = doc.document.text().len_lines().saturating_sub(1);
            let line = line.min(last);
            doc.editor.place_caret(line, 0);
            doc.editor.top_line = line.saturating_sub(3);
            doc.editor.top_segment = (doc.editor.top_line, 0);
        }
        screen.refresh_marks();
    }

    /// 選んでいるかたまりを、左・右・両方で決める（結果の行を入れ替える。取り消し 1 回で戻る）
    fn merge_take(&mut self, choice: Take) {
        let Some(mut screen) = self.merge_screen.take() else {
            return;
        };
        let Some(n) = screen.current else {
            self.merge_screen = Some(screen);
            return;
        };
        let region = screen.regions[n].clone();
        let new = merge::take(&region, &screen.left_text, &screen.right_text, choice);
        let rope = screen.result.document.text();
        let last = rope.len_lines();
        let start = rope.line_to_byte(region.result.start.min(last));
        let end = rope.line_to_byte(region.result.end.min(last));
        let removed = rope.byte_slice(start..end).to_string();
        self.in_doc(&mut screen.result, false, |app| {
            app.replace_range(start..end, removed, new.clone());
            app.editor.anchor = None;
        });
        // 決めたかたまりは、入れた行そのもの。後ろのかたまりは行の差だけずらす
        let added = crate::diff::lines(&new).len().saturating_sub(1)
            + usize::from(!new.is_empty() && !new.ends_with('\n'));
        let delta = added as isize - region.result.len() as isize;
        for (index, other) in screen.regions.iter_mut().enumerate() {
            if index == n {
                other.result = region.result.start..region.result.start + added;
            } else if other.result.start >= region.result.end && index > n {
                other.result.start = (other.result.start as isize + delta) as usize;
                other.result.end = (other.result.end as isize + delta) as usize;
            }
        }
        screen.resolved[n] = true;
        screen.refresh_marks();
        self.merge_screen = Some(screen);
        // 残っている衝突があれば次へ
        if self
            .merge_screen
            .as_ref()
            .is_some_and(|screen| screen.remaining() > 0)
        {
            self.merge_step(true);
        }
    }

    /// 保存する（保存先を聞く）。**衝突が残っていれば先に確かめる**
    fn merge_save(&mut self, confirmed: bool) -> Task<Message> {
        let Some(screen) = self.merge_screen.as_mut() else {
            return Task::none();
        };
        if screen.remaining() > 0 && !confirmed {
            screen.confirm_save = true;
            return Task::none();
        }
        let directory = screen.base_path.parent().map(Path::to_path_buf);
        let name = file_name(&screen.base_path);
        let extension = screen
            .base_path
            .extension()
            .map_or("md".to_owned(), |ext| ext.to_string_lossy().into_owned());
        let picker = super::picker::Picker::save("文書", &extension, name).in_directory(directory);
        self.ask(picker)
            .map(|path| Message::Merge(MergeMessage::SaveTo(path)))
    }

    /// 結果を書く
    fn merge_write(&mut self, path: &Path) {
        let Some(screen) = self.merge_screen.as_mut() else {
            return;
        };
        let text = screen.result.document.text().to_string();
        match crate::io::save(path, &text, &screen.result.meta.format) {
            Ok(()) => {
                screen.result.meta.dirty = false;
                screen.result.meta.path = Some(path.to_path_buf());
                self.notice = Some(super::notice::Notice::plain(format!(
                    "マージの結果を保存しました: {}",
                    path.display()
                )));
            }
            Err(error) => self.notice = Some(super::notice::Notice::plain(error.to_string())),
        }
    }

    /// マージの画面
    pub(super) fn merge_view<'a>(&'a self, screen: &'a MergeScreen) -> Element<'a, Message> {
        let conflicts = screen.conflicts();
        let remaining = screen.remaining();
        let summary = if conflicts == 0 {
            format!(
                "衝突はありません（取り込んだ変更 {} か所）",
                screen.regions.len()
            )
        } else {
            format!("衝突 {conflicts} か所（残り {remaining}）")
        };
        let position = screen
            .current
            .map(|n| format!("{} / {}", n + 1, screen.regions.len()))
            .unwrap_or_default();
        let small = |label: &'static str, message: Option<MergeMessage>| {
            button(text(label).size(12))
                .padding([4, 10])
                .on_press_maybe(message.map(Message::Merge))
        };
        let any = !screen.regions.is_empty();
        let chosen = screen.current.is_some();
        let take = |label, choice| small(label, chosen.then_some(MergeMessage::Take(choice)));
        let header = row![
            text("3 つのファイルをマージ").size(15),
            text(summary).size(12).width(Length::Fill),
            text(position).size(12),
            small("▲ 前の衝突", any.then_some(MergeMessage::Previous)),
            small("▼ 次の衝突", any.then_some(MergeMessage::Next)),
            take("左を取る", Take::Left),
            take("右を取る", Take::Right),
            take("両方（左が先）", Take::LeftThenRight),
            take("両方（右が先）", Take::RightThenLeft),
            small("保存…", Some(MergeMessage::Save)),
            button(text("閉じる").size(12))
                .padding([4, 12])
                .on_press(Message::Merge(MergeMessage::Close)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let mut body = column![header].spacing(6).padding(6);
        let confirm =
            |message: String, yes: &'static str, on_yes: MergeMessage, on_no: MergeMessage| {
                row![
                    text(message).size(12),
                    button(text(yes).size(12))
                        .padding([4, 10])
                        .on_press(Message::Merge(on_yes)),
                    button(text("戻る").size(12))
                        .padding([4, 10])
                        .on_press(Message::Merge(on_no)),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
            };
        if screen.confirm_save {
            body = body.push(confirm(
                format!("衝突が {remaining} か所残っています。このまま保存しますか。"),
                "このまま保存する",
                MergeMessage::ConfirmSave(true),
                MergeMessage::ConfirmSave(false),
            ));
        }
        if screen.confirm_close {
            body = body.push(confirm(
                "結果を保存していません。閉じると失われます。".to_owned(),
                "保存せずに閉じる",
                MergeMessage::ConfirmClose(true),
                MergeMessage::ConfirmClose(false),
            ));
        }
        let pane = |which: MergePane, title: String, index: usize| -> Element<'a, Message> {
            let doc = screen.doc(which);
            let (size, line_height) = self.editor_sizes();
            let view = EditorView::new(&doc.document, &doc.editor, move |action| {
                Message::Merge(MergeMessage::Pane(which, action))
            })
            .font(self.editor_font())
            .sizes(size, line_height)
            .diff_marks(Some(&screen.marks[index]))
            .highlight_matches(self.search_highlight(super::SearchTarget::Merge(which)))
            .match_colors(self.match_colors())
            .accept_keys(
                !self.search_focused
                    && screen.focus == which
                    && self.open_menu.is_none()
                    && !screen.confirm_close
                    && !screen.confirm_save,
            )
            .display(
                self.tab_width(),
                self.settings.show_invisibles,
                self.settings.show_gremlins,
                doc.meta.format.line_ending == crate::io::LineEnding::Crlf,
            );
            column![
                container(text(title).size(12)).padding([2, 4]),
                container(view).width(Length::Fill).height(Length::Fill),
            ]
            .width(Length::FillPortion(1))
            .into()
        };
        let mut result_title = "結果（直せる）".to_owned();
        if screen.result.meta.dirty {
            result_title.push_str(" ●");
        }
        body.push(
            row![
                pane(
                    MergePane::Left,
                    format!("{}（読むだけ）", screen.left_title),
                    0
                ),
                pane(MergePane::Result, result_title, 1),
                pane(
                    MergePane::Right,
                    format!("{}（読むだけ）", screen.right_title),
                    2
                ),
            ]
            .spacing(4)
            .height(Length::Fill),
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "一\n二\n三\n四\n五\n";
    const LEFT: &str = "一\n二（左）\n三\n四\n五（左）\n";
    const RIGHT: &str = "一\n二（右）\n三\n四\n五\n";

    fn loaded(path: &str, text: &str) -> LoadedFile {
        LoadedFile {
            path: PathBuf::from(path),
            text: text.to_owned(),
            format: Default::default(),
            oversized: false,
            lossy: false,
            truncated: None,
        }
    }

    fn app() -> App {
        let mut app = App::new().0;
        app.replace_document(String::new(), DocumentMeta::untitled());
        app.open_merge(
            loaded("base.md", BASE),
            loaded("left.md", LEFT),
            loaded("right.md", RIGHT),
        );
        app
    }

    fn result(app: &App) -> String {
        app.merge_screen
            .as_ref()
            .unwrap()
            .result
            .document
            .text()
            .to_string()
    }

    /// 片側だけの変更は取り込み、衝突は共通の元のまま。最初の衝突を選んでいる
    #[test]
    fn opening_merges_and_selects_the_first_conflict() {
        let app = app();
        let screen = app.merge_screen.as_ref().unwrap();
        assert_eq!(result(&app), "一\n二\n三\n四\n五（左）\n");
        assert_eq!((screen.conflicts(), screen.remaining()), (1, 1));
        assert_eq!(screen.current, Some(0));
        assert_eq!(screen.result.editor.cursor_line, 1);
        assert_eq!(screen.marks[1].lines[1], LineMark::Changed);
        assert_eq!(screen.marks[1].lines[4], LineMark::Only);
    }

    /// 取ると結果の行が入れ替わり、衝突が決まる。取り消し 1 回で戻る
    #[test]
    fn taking_a_side_resolves_the_conflict() {
        let mut app = app();
        let _ = app.update(Message::Merge(MergeMessage::Take(Take::RightThenLeft)));
        assert_eq!(result(&app), "一\n二（右）\n二（左）\n三\n四\n五（左）\n");
        let screen = app.merge_screen.as_ref().unwrap();
        assert_eq!(screen.remaining(), 0);
        assert_eq!(screen.regions[0].result, 1..3);
        assert_eq!(
            screen.regions[1].result,
            5..6,
            "後ろのかたまりは 1 行ずれる"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(result(&app), "一\n二\n三\n四\n五（左）\n");
    }

    /// 衝突の中を手で直すと、手で決めたものとみなす。前の行を足すと後ろのかたまりがずれる
    #[test]
    fn editing_by_hand_shifts_and_resolves() {
        let mut app = app();
        let screen = app.merge_screen.as_mut().unwrap();
        screen.shift_after_edit(
            "一\n二\n三\n四\n五（左）\n",
            "零\n一\n二\n三\n四\n五（左）\n",
        );
        assert_eq!(screen.regions[0].result, 2..3);
        assert_eq!(screen.regions[1].result, 5..6);
        assert!(!screen.resolved[0]);
        screen.shift_after_edit(
            "零\n一\n二\n三\n四\n五（左）\n",
            "零\n一\n2\n三\n四\n五（左）\n",
        );
        assert!(screen.resolved[0], "衝突の中を直したら決めたもの");
        let _ = &mut app;
    }

    /// 衝突が残ったまま保存しようとすると確かめる。保存していなければ閉じる前に確かめる
    #[test]
    fn saving_and_closing_ask_first() {
        let mut app = app();
        let _ = app.update(Message::Merge(MergeMessage::Save));
        assert!(app.merge_screen.as_ref().unwrap().confirm_save);
        let _ = app.update(Message::Merge(MergeMessage::ConfirmSave(false)));
        let path = std::env::temp_dir().join(format!("mdview-merge-{}.md", std::process::id()));
        let _ = app.update(Message::Merge(MergeMessage::SaveTo(Some(path.clone()))));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), result(&app));
        assert!(!app.merge_screen.as_ref().unwrap().result.meta.dirty);
        let _ = app.update(Message::Merge(MergeMessage::Close));
        assert!(app.merge_screen.is_none(), "保存したら確かめずに閉じる");
        let _ = std::fs::remove_file(path);
        let mut app = super::tests::app();
        let _ = app.update(Message::Merge(MergeMessage::Close));
        assert!(app.merge_screen.as_ref().unwrap().confirm_close);
    }
}
