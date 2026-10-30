//! WYSIWYG 編集（v2.5.0 R-01 a）。描いた文字の上でキャレットを動かし、打ち、消す。
//!
//! **キャレットは編集画面と同じもの**（原文のバイト位置）を使う。打つ・貼る・取り消す・自動保存・リストの続きは
//! いままでの道をそのまま通る。ここが受け持つのは、描いた文字を単位にした動きと消し方だけ
//! （対応の表は `crate::edit::wysiwyg`）。見た目の行が要る動き（上下・行の頭と終わり・押した所）は
//! プレビューの部品が決めて `Place` で知らせる。

use super::fluent;
use super::App;
use crate::edit::wysiwyg::{map_block, BlockMap, Pos};
use crate::render::{Action, WysiwygAction};

/// キャレットのあるブロック（添字・その原文の頭・対応）
struct Here {
    index: usize,
    start: usize,
    source: String,
    map: BlockMap,
    pos: Pos,
}

impl App {
    /// WYSIWYG に入れる大きさか（設定の上限。KB）
    pub(super) fn wysiwyg_fits(&self) -> bool {
        self.document.text().len_bytes() <= self.settings.wysiwyg_limit_kb as usize * 1024
    }

    /// WYSIWYG に入れない文書なら分割に戻して知らせる（`update` の出口）
    pub(super) fn guard_wysiwyg(&mut self) {
        let mode = crate::render::ViewMode::Wysiwyg;
        if self.mode != mode
            || (self.settings.wysiwyg && self.wysiwyg_fits() && !self.document.is_plain())
        {
            return;
        }
        self.mode = crate::render::ViewMode::Split;
        self.notice = Some(super::notice::Notice::plain(format!(
            "この文書は WYSIWYG で直せないので、分割の表示に戻しました（{} KB まで。テキストとして読む文書は対象外）",
            self.settings.wysiwyg_limit_kb
        )));
    }

    /// 原文のブロックの対応（直せないブロックは `None`）
    fn map_of(&self, index: usize) -> Option<(usize, String, BlockMap)> {
        let block = self.document.blocks().get(index)?;
        // 欄に出している frontmatter には入らない（v2.6.0 R-03）
        if self
            .front_hidden_end()
            .is_some_and(|end| block.bytes.end <= end)
        {
            return None;
        }
        let source = self
            .document
            .text()
            .byte_slice(block.bytes.clone())
            .to_string();
        let map = map_block(block, &source)?;
        Some((block.bytes.start, source, map))
    }

    /// キャレットのあるブロック（ブロックの外・直せないブロックなら `None`）
    fn here(&self) -> Option<Here> {
        let byte = self.caret_byte();
        let index = self.document.block_at_byte(byte)?;
        let (start, source, map) = self.map_of(index)?;
        let pos = map.pos_of(byte.saturating_sub(start));
        Some(Here {
            index,
            start,
            source,
            map,
            pos,
        })
    }

    /// `index` から（`index` を含めて）`forward` の向きに、最初の直せるブロックの頭（後ろ向きなら終わり）の原文の位置
    fn editable_from(&self, index: isize, forward: bool) -> Option<usize> {
        let count = self.document.blocks().len() as isize;
        let mut index = index;
        while (0..count).contains(&index) {
            if let Some((start, _, map)) = self.map_of(index as usize) {
                let pos = if forward { map.first() } else { map.last() };
                return Some(start + map.byte_of(pos));
            }
            index += if forward { 1 } else { -1 };
        }
        None
    }

    /// 直せる隣のブロック（`forward` なら後ろ）の頭（か終わり）
    pub(super) fn neighbour(&self, from: usize, forward: bool) -> Option<usize> {
        let from = from as isize;
        self.editable_from(if forward { from + 1 } else { from - 1 }, forward)
    }

    /// キャレットを原文の位置へ置く（`select` なら選んだまま）
    pub(super) fn wysiwyg_place(&mut self, byte: usize, select: bool) {
        if select {
            if self.editor.anchor.is_none() {
                self.editor.anchor = Some(self.caret_byte());
            }
        } else {
            self.editor.anchor = None;
        }
        let byte = byte.min(self.document.text().len_bytes());
        let (line, column) = self.document.position_at(byte);
        if select {
            self.editor.extend_caret(line, column);
        } else {
            self.editor.place_caret(line, column);
        }
    }

    /// 描いた字 1 つぶん左・右へ（ブロックの端なら隣の直せるブロックへ）
    fn step(&self, forward: bool) -> Option<usize> {
        let Some(here) = self.here() else {
            // ブロックの間（空行）にいる。隣のブロックへ
            let byte = self.caret_byte();
            let index = self
                .document
                .blocks()
                .partition_point(|block| block.bytes.end <= byte) as isize;
            return if forward {
                self.editable_from(index, true)
            } else {
                self.editable_from(index - 1, false)
            };
        };
        // **原文の位置が変わるまで進める**（広げたタブの空白のように、同じ位置に当たる字の上では止まらない。v2.8.1 R-02）
        let here_byte = here.start + here.map.byte_of(here.pos);
        let mut pos = here.pos;
        loop {
            let next = if forward {
                here.map.after(pos)
            } else {
                here.map.before(pos)
            };
            match next {
                Some(found) => {
                    let byte = here.start + here.map.byte_of(found);
                    if byte != here_byte {
                        return Some(byte);
                    }
                    pos = found;
                }
                None => return self.neighbour(here.index, forward),
            }
        }
    }

    /// WYSIWYG の操作を当てる
    pub(super) fn wysiwyg(&mut self, action: WysiwygAction) {
        match action {
            WysiwygAction::Place { byte, select } => self.wysiwyg_place(byte, select),
            WysiwygAction::Left { select } | WysiwygAction::Right { select } => {
                let forward = matches!(action, WysiwygAction::Right { .. });
                // 選んでいて、選んだまま動かすのでなければ、選んだ端へ寄せるだけ
                if !select {
                    if let Some(range) = self.selected() {
                        let to = if forward { range.end } else { range.start };
                        self.wysiwyg_place(to, false);
                        return;
                    }
                }
                if let Some(byte) = self.step(forward) {
                    self.wysiwyg_place(byte, select);
                }
            }
            WysiwygAction::Backspace | WysiwygAction::Delete => {
                if self.refuse_edit() {
                    return;
                }
                let forward = matches!(action, WysiwygAction::Delete);
                if let Some(range) = self.selected() {
                    // 表のセルをまたぐなら、セルの中身だけを消す（v2.5.1 R-01）
                    if !self.clear_cells(range.clone()) {
                        self.delete_source(range);
                    }
                    return;
                }
                self.delete_char(forward);
            }
            WysiwygAction::Enter => {
                if self.refuse_edit() {
                    return;
                }
                let kind = self.here().and_then(|here| {
                    self.document
                        .blocks()
                        .get(here.index)
                        .map(|b| b.kind.clone())
                });
                // HTML の表も表（v2.8.0 R-05。段落として読んでいる）
                let html = self.here().is_some_and(|here| here.map.cells)
                    && kind == Some(crate::parse::BlockKind::Paragraph);
                match kind {
                    // 表は下のセルへ（v2.5.1 R-01。セルの中では改行しない）
                    Some(crate::parse::BlockKind::Table) if self.table_enter() => {}
                    Some(crate::parse::BlockKind::Table) => {}
                    _ if html => {
                        let _ = self.table_enter();
                    }
                    // リスト・引用の続き、コードの改行は、編集画面と同じ道（v2.5.0 R-01 b で引用も）
                    Some(crate::parse::BlockKind::List | crate::parse::BlockKind::Quote) => {
                        let _ = self.apply(Action::Insert("\n".to_owned()));
                    }
                    Some(crate::parse::BlockKind::Code { .. }) => self.insert("\n"),
                    // 段落・見出しは、新しい段落に分ける
                    _ => self.insert("\n\n"),
                }
            }
            WysiwygAction::Insert(text) => {
                if !self.refuse_edit() {
                    // 表のセルをまたいで選んでいれば、先にセルの中身を消す（v2.5.1 R-01）
                    if let Some(range) = self.selected() {
                        self.clear_cells(range);
                    }
                    self.insert(&text);
                }
            }
            // Tab は `update` が字下げか編集画面と同じ道へ渡す
            WysiwygAction::Tab { .. } => {}
            WysiwygAction::ToggleTask { byte } => self.toggle_task_at(byte),
            WysiwygAction::CodeLanguage(language) => self.set_code_language(&language),
            WysiwygAction::Escape => self.leave_figure(),
        }
    }

    /// キャレットのあるコードブロックの言語（コードブロックの外なら `None`）
    pub(super) fn code_language(&self) -> Option<String> {
        let here = self.here()?;
        match &self.document.blocks().get(here.index)?.kind {
            crate::parse::BlockKind::Code { language } => {
                Some(language.clone().unwrap_or_default())
            }
            _ => None,
        }
    }

    /// コードブロックの言語を変える（開きのフェンスの後ろを書き換える。v2.5.0 R-01 b）
    fn set_code_language(&mut self, language: &str) {
        if self.code_language().is_none() || self.refuse_edit() {
            return;
        }
        let Some(here) = self.here() else {
            return;
        };
        let fence = here
            .source
            .split('\n')
            .next()
            .unwrap_or("")
            .trim_end_matches('\r');
        let trimmed = fence.trim_start();
        let lead = &fence[..fence.len() - trimmed.len()];
        let Some(mark) = trimmed.chars().next().filter(|c| matches!(c, '`' | '~')) else {
            return;
        };
        let count = trimmed.chars().take_while(|c| *c == mark).count();
        // 言語の欄に空白・制御文字は入れない（フェンスの行が壊れる）
        let language: String = language
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .collect();
        let new = format!("{lead}{}{language}", mark.to_string().repeat(count));
        if new == fence {
            return;
        }
        let caret = self.caret_byte();
        let anchor = self.editor.anchor;
        let old_len = fence.len();
        let new_len = new.len();
        self.replace_range(here.start..here.start + old_len, fence.to_owned(), new);
        // キャレットはフェンスより後ろ（中身）にある。長さの差だけずらす
        let shift = |byte: usize| (byte + new_len).saturating_sub(old_len);
        self.wysiwyg_place(shift(caret), false);
        self.editor.anchor = anchor.map(shift);
    }

    /// 書式の道具の帯（太字・斜体・コード・リンク。編集画面の Ctrl+B などと同じ）。
    /// **アイコンのボタン**で、ポインタを乗せると名前と打鍵を出す
    pub(super) fn wysiwyg_toolbar(&self) -> iced::Element<'_, crate::render::Message> {
        use super::features::FormatKind;
        use super::keymap::Command;
        use crate::render::Message;
        use crate::render::Restructure as R;
        use iced::widget::{button, container, image, text, tooltip};
        let color = self.theme().palette().text;
        let tool = |icon: Icon, name: &'static str, command: Command, message: Message| {
            let accel = self.keymap.accel(command);
            let tip = if accel.is_empty() {
                name.to_owned()
            } else {
                format!("{name}（{accel}）")
            };
            tooltip(
                button(
                    image(icon_handle(icon, color))
                        .width(iced::Length::Fixed(18.0))
                        .height(iced::Length::Fixed(18.0)),
                )
                .padding(5)
                .style(button::text)
                .on_press(message),
                container(text(tip).size(11))
                    .padding([3, 8])
                    .style(container::bordered_box),
                tooltip::Position::Bottom,
            )
            .into()
        };
        let format = |icon, name, command, kind: FormatKind| {
            tool(icon, name, command, Message::Format(kind))
        };
        let shape = |icon, command: Command, kind: R| {
            tool(icon, command.label(), command, Message::Restructure(kind))
        };
        // 区切り（道具の組の間）
        let gap = || -> iced::Element<'_, Message> {
            container(text(""))
                .width(iced::Length::Fixed(1.0))
                .height(iced::Length::Fixed(18.0))
                .style(container::bordered_box)
                .into()
        };
        let mut tools: Vec<iced::Element<'_, Message>> = vec![
            format(Icon::Bold, "太字", Command::Bold, FormatKind::Bold),
            format(Icon::Italic, "斜体", Command::Italic, FormatKind::Italic),
            format(
                Icon::Code,
                "インラインコード",
                Command::InlineCode,
                FormatKind::Code,
            ),
            format(Icon::Link, "リンク", Command::Link, FormatKind::Link),
            gap(),
            // リスト・見出しの組み替え（v2.5.0 R-01 b）
            shape(
                Icon::Outdent,
                Command::ItemOutdent,
                R::Indent { outdent: true },
            ),
            shape(
                Icon::Indent,
                Command::ItemIndent,
                R::Indent { outdent: false },
            ),
            shape(Icon::Task, Command::ToggleTask, R::ToggleTask),
            gap(),
            shape(
                Icon::HeadingUp,
                Command::PromoteHeading,
                R::Heading { promote: true },
            ),
            shape(
                Icon::HeadingDown,
                Command::DemoteHeading,
                R::Heading { promote: false },
            ),
            shape(
                Icon::SectionUp,
                Command::SectionUp,
                R::MoveSection { up: true },
            ),
            shape(
                Icon::SectionDown,
                Command::SectionDown,
                R::MoveSection { up: false },
            ),
        ];
        // 表の中なら、行・列を足す／消す、列の寄せ方（v2.5.1 R-01）
        if self.caret_in_table() {
            use crate::edit::table::Align;
            use crate::render::TableEdit as T;
            let table = |icon, edit: T| {
                tool(
                    icon,
                    edit.label(),
                    Command::Table(edit),
                    Message::TableEdit(edit),
                )
            };
            tools.push(gap());
            tools.push(table(Icon::RowAbove, T::InsertRow { below: false }));
            tools.push(table(Icon::RowBelow, T::InsertRow { below: true }));
            tools.push(table(Icon::RowDelete, T::DeleteRow));
            tools.push(table(Icon::ColumnLeft, T::InsertColumn { right: false }));
            tools.push(table(Icon::ColumnRight, T::InsertColumn { right: true }));
            tools.push(table(Icon::ColumnDelete, T::DeleteColumn));
            tools.push(gap());
            tools.push(table(Icon::AlignLeft, T::Align(Align::Left)));
            tools.push(table(Icon::AlignCenter, T::Align(Align::Center)));
            tools.push(table(Icon::AlignRight, T::Align(Align::Right)));
            // HTML の表なら、セルをつなぐ・分ける（v2.9.0 R-01）
            if self.caret_in_html_table() {
                tools.push(gap());
                tools.push(table(Icon::Merge, T::MergeCells));
                tools.push(table(Icon::Split, T::SplitCell));
            }
        }
        // コードブロックの中なら、言語の欄（v2.5.0 R-01 b）
        if let Some(language) = self.code_language() {
            tools.push(gap());
            tools.push(text("言語").size(12).into());
            tools.push(
                fluent::text_input("なし", &language)
                    .on_input(|value| {
                        Message::Preview(crate::render::PreviewAction::Wysiwyg(
                            WysiwygAction::CodeLanguage(value),
                        ))
                    })
                    .size(12)
                    .padding([2, 6])
                    .width(iced::Length::Fixed(120.0))
                    .into(),
            );
        }
        iced::widget::Row::with_children(tools)
            .spacing(2)
            .padding([2, 8])
            .align_y(iced::Alignment::Center)
            .into()
    }

    /// 原文の範囲を消してキャレットをその頭へ
    fn delete_source(&mut self, range: std::ops::Range<usize>) {
        if range.is_empty() || range.end > self.document.text().len_bytes() {
            return;
        }
        let removed = self.document.text().byte_slice(range.clone()).to_string();
        self.replace_range(range.clone(), removed, String::new());
        self.wysiwyg_place(range.start, false);
    }

    /// 描いた字 1 つを消す。**ブロックの頭で後ろへ消すと、前のブロックとつなぐ**（見出しなら先に段落へ戻す）
    fn delete_char(&mut self, forward: bool) {
        let Some(here) = self.here() else {
            // ブロックの間（空行）。原文の 1 字を消す（編集画面と同じ）
            if forward {
                let _ = self.apply(Action::Delete);
            } else {
                self.backspace();
            }
            return;
        };
        let target = if forward {
            Some(here.pos)
        } else {
            here.map.before(here.pos)
        };
        let table = here.map.cells;
        if let Some(pos) = target {
            // 行をまたぐ（リストの項目の境）なら、前の行の終わりからこの行の頭までを消す
            if pos.line != here.pos.line || (forward && pos.index >= here.map.lines[pos.line].len())
            {
                // **表のセルの境は消さない**（v2.5.1 R-01。`|` が消えると表が崩れる）
                if table {
                    return;
                }
                let (from, to) = if forward {
                    let next = here.map.after(pos);
                    (here.map.byte_of(pos), next.map(|n| here.map.byte_of(n)))
                } else {
                    (here.map.byte_of(pos), Some(here.map.byte_of(here.pos)))
                };
                if let Some(to) = to {
                    self.delete_source(here.start + from..here.start + to);
                    return;
                }
            } else if let Some(range) = here.map.char_range(&here.source, pos) {
                self.delete_source(here.start + range.start..here.start + range.end);
                return;
            } else {
                // 原文に無い字（画像の目印など）。飛ばしてキャレットだけ動かす
                self.wysiwyg_place(here.start + here.map.byte_of(pos), false);
                return;
            }
        }
        // ブロックの端
        let kind = self.document.blocks()[here.index].kind.clone();
        // **つなげるのは段落・見出し・リスト・引用どうしだけ**（v2.5.1 R-01）。表・コード・図とは
        // つながず、隣のブロックへキャレットを動かすだけにする（囲みや `|` が消えると崩れる）
        let joinable = |kind: &crate::parse::BlockKind| {
            matches!(
                kind,
                crate::parse::BlockKind::Paragraph
                    | crate::parse::BlockKind::Heading(_)
                    | crate::parse::BlockKind::List
                    | crate::parse::BlockKind::Quote
            )
        };
        let neighbour_joinable = |byte: usize| {
            self.document
                .block_at_byte(byte)
                .and_then(|index| self.document.blocks().get(index))
                .is_some_and(|block| joinable(&block.kind))
        };
        if !forward && here.pos == here.map.first() && !joinable(&kind) {
            if let Some(previous) = self.neighbour(here.index, false) {
                self.wysiwyg_place(previous, false);
            }
            return;
        }
        if forward && !joinable(&kind) {
            if let Some(next) = self.neighbour(here.index, true) {
                self.wysiwyg_place(next, false);
            }
            return;
        }
        if !forward {
            let caret = here.start + here.map.byte_of(here.pos);
            // 見出し・リスト・引用の頭なら、まず記号を外して段落にする
            if matches!(
                kind,
                crate::parse::BlockKind::Heading(_)
                    | crate::parse::BlockKind::List
                    | crate::parse::BlockKind::Quote
            ) && here.pos == here.map.first()
                && caret > here.start
            {
                self.delete_source(here.start..caret);
                return;
            }
            // 前のブロックの終わりから、このブロックの字の頭までを消してつなぐ
            if let Some(previous) = self.neighbour(here.index, false) {
                if neighbour_joinable(previous) {
                    self.delete_source(previous..caret);
                } else {
                    self.wysiwyg_place(previous, false);
                }
            }
        } else if let Some(next) = self.neighbour(here.index, true) {
            let caret = here.start + here.map.byte_of(here.pos);
            if neighbour_joinable(next) {
                self.delete_source(caret..next);
            } else {
                self.wysiwyg_place(next, false);
            }
        }
    }

    /// WYSIWYG の様子（試験の口）
    pub(super) fn wysiwyg_state(&self) -> serde_json::Value {
        match self.here() {
            Some(here) => {
                let mut state = serde_json::json!({
                    "block": here.index,
                    "line": here.pos.line,
                    "index": here.pos.index,
                    "text": here.map.text(),
                });
                // 表のセル（v2.5.1 R-01）と、小窓を開いている図・数式
                if let Some(columns) = here.map.columns {
                    state["row"] = (here.pos.line / columns).into();
                    state["column"] = (here.pos.line % columns).into();
                }
                state["figure"] = self.figure_hold.is_some().into();
                state
            }
            None => serde_json::Value::Null,
        }
    }
}

/// 道具の帯のアイコン（`assets/toolbar/*.svg`。**差し替えれば絵が変わる**。`currentColor` は字の色で描く）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Icon {
    Bold,
    Italic,
    Code,
    Link,
    // 組み替え（v2.5.0 R-01 b）
    Indent,
    Outdent,
    Task,
    HeadingUp,
    HeadingDown,
    SectionUp,
    SectionDown,
    // 表（v2.5.1 R-01）
    RowAbove,
    RowBelow,
    RowDelete,
    ColumnLeft,
    ColumnRight,
    ColumnDelete,
    AlignLeft,
    AlignCenter,
    AlignRight,
    // セルをつなぐ・分ける（v2.9.0 R-01）
    Merge,
    Split,
}

impl Icon {
    const ALL: [Icon; 22] = [
        Icon::Bold,
        Icon::Italic,
        Icon::Code,
        Icon::Link,
        Icon::Indent,
        Icon::Outdent,
        Icon::Task,
        Icon::HeadingUp,
        Icon::HeadingDown,
        Icon::SectionUp,
        Icon::SectionDown,
        Icon::RowAbove,
        Icon::RowBelow,
        Icon::RowDelete,
        Icon::ColumnLeft,
        Icon::ColumnRight,
        Icon::ColumnDelete,
        Icon::AlignLeft,
        Icon::AlignCenter,
        Icon::AlignRight,
        Icon::Merge,
        Icon::Split,
    ];

    fn svg(self) -> &'static str {
        match self {
            Self::Indent => include_str!("../../assets/toolbar/indent.svg"),
            Self::Outdent => include_str!("../../assets/toolbar/outdent.svg"),
            Self::Task => include_str!("../../assets/toolbar/task.svg"),
            Self::HeadingUp => include_str!("../../assets/toolbar/heading-up.svg"),
            Self::HeadingDown => include_str!("../../assets/toolbar/heading-down.svg"),
            Self::SectionUp => include_str!("../../assets/toolbar/section-up.svg"),
            Self::SectionDown => include_str!("../../assets/toolbar/section-down.svg"),
            Self::Bold => include_str!("../../assets/toolbar/bold.svg"),
            Self::Italic => include_str!("../../assets/toolbar/italic.svg"),
            Self::Code => include_str!("../../assets/toolbar/code.svg"),
            Self::Link => include_str!("../../assets/toolbar/link.svg"),
            Self::RowAbove => include_str!("../../assets/toolbar/table-row-above.svg"),
            Self::RowBelow => include_str!("../../assets/toolbar/table-row-below.svg"),
            Self::RowDelete => include_str!("../../assets/toolbar/table-row-delete.svg"),
            Self::ColumnLeft => include_str!("../../assets/toolbar/table-column-left.svg"),
            Self::ColumnRight => include_str!("../../assets/toolbar/table-column-right.svg"),
            Self::ColumnDelete => include_str!("../../assets/toolbar/table-column-delete.svg"),
            Self::AlignLeft => include_str!("../../assets/toolbar/align-left.svg"),
            Self::AlignCenter => include_str!("../../assets/toolbar/align-center.svg"),
            Self::AlignRight => include_str!("../../assets/toolbar/align-right.svg"),
            Self::Merge => include_str!("../../assets/toolbar/table-merge.svg"),
            Self::Split => include_str!("../../assets/toolbar/table-split.svg"),
        }
    }
}

/// アイコンの画素（字の色ごとに 1 度だけ描く。**画面の 2 倍の細かさ**で描いて縮めて出す）
fn icon_handle(icon: Icon, color: iced::Color) -> iced::widget::image::Handle {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<(Icon, [u8; 4]), iced::widget::image::Handle>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let rgba = color.into_rgba8();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(handle) = cache
        .lock()
        .ok()
        .and_then(|map| map.get(&(icon, rgba)).cloned())
    {
        return handle;
    }
    let hex = format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]);
    let svg = icon.svg().replace("currentColor", &hex);
    let handle = match crate::embed::diagram::rasterize_scaled(&svg, 2.0, 96) {
        Ok(picture) => iced::widget::image::Handle::from_rgba(
            picture.width,
            picture.height,
            picture.pixels.as_ref().clone(),
        ),
        Err(_) => iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
    };
    if let Ok(mut map) = cache.lock() {
        map.insert((icon, rgba), handle.clone());
    }
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    /// アイコンはどれも描ける（SVG が読める）
    #[test]
    fn every_toolbar_icon_rasterizes() {
        for icon in Icon::ALL {
            let svg = icon.svg().replace("currentColor", "#000000");
            let picture = crate::embed::diagram::rasterize_scaled(&svg, 2.0, 96).expect("描ける");
            assert_eq!((picture.width, picture.height), (48, 48), "{icon:?}");
            assert!(
                picture.pixels.iter().skip(3).step_by(4).any(|a| *a > 0),
                "{icon:?} が空"
            );
        }
    }
}
