//! diff の画面（v2.3.0 R-02）。
//!
//! **本文を覆う画面**（設定画面と同じ）に、2 つの文書を左右に並べる。
//! 比べる計算は `crate::diff`、空きの段と色は編集部品（`EditorView::spacers` /
//! `diff_marks`）が受け持つ。
//!
//! # 左右の面の編集
//!
//! **左右の面とも、編集画面と同じ道で書き換える**（要件定義書 R-02 D-1）。打鍵の処理
//! （`App::apply` ほか）は本文（`self.document`）1 つを相手に作ってあるので、面を触る
//! ときだけ、本文・キャレット・履歴・素性を面のものと**入れ替えて**同じ処理を通す
//! （[`App::in_pane`]）。入れ替えている間は `diff_own` を立て、退避・自動保存・目次など
//! 本文にしか関わらない後始末を止める。
//!
//! 「保存してある元と比べる」の右の面は、**編集中の文書そのもの**（D-9）。入れ替えずに
//! 本文を直接書き換えるので、退避や自動保存はふだんどおり働く。

use super::fluent;
use std::path::PathBuf;

use iced::widget::{button, column, container, row, text};
use iced::{Color, Element, Length, Task};

use super::file::DocumentMeta;
use super::App;
use crate::diff::{Block, Diff, Options, Side};
use crate::document::{history::History, Document};
use crate::io::LoadedFile;
use crate::render::wrap::Wrap;
use crate::render::{Action, DiffMarks, EditorState, EditorView, FileCommand, LineMark, Message};

/// これ以下の大きさ（左右の合計バイト）なら、**その場で比べ直す**。
/// 超えたら裏の糸で比べる（10 MB 同士は性能の目標に入れない。要件定義書 §0.2）
const SYNC_LIMIT: usize = 512 * 1024;

/// 行の中の違いを求める行の組の上限。**超えたぶんは行ごとの色だけにする**
const INLINE_PAIRS: usize = 2_000;

/// 色（明るい配色でも暗い配色でも読めるよう、半透明で重ねる）
const REMOVED: Color = Color::from_rgba(0.90, 0.25, 0.25, 0.22);
const ADDED: Color = Color::from_rgba(0.20, 0.70, 0.30, 0.22);
const CHANGED: Color = Color::from_rgba(0.95, 0.75, 0.10, 0.20);
const INLINE: Color = Color::from_rgba(0.95, 0.55, 0.05, 0.40);
const SPACER: Color = Color::from_rgba(0.50, 0.50, 0.50, 0.12);

/// diff の画面へのメッセージ。
#[derive(Debug, Clone)]
pub enum DiffMessage {
    /// 「ファイルを比べる」。`in_app` ならアプリ内で選ぶ
    OpenFiles {
        in_app: bool,
    },
    /// 左（1 つ目）を選んだ
    PickedLeft {
        path: Option<PathBuf>,
        in_app: bool,
    },
    /// 右（2 つ目）を選んだ
    PickedRight {
        left: PathBuf,
        path: Option<PathBuf>,
    },
    LoadedFiles(Result<(Box<LoadedFile>, Box<LoadedFile>), String>),
    /// 「保存してある元と比べる」
    OpenSaved,
    LoadedSaved(Result<Box<LoadedFile>, String>),
    /// 面の編集部品からの通知
    Pane(Side, Action),
    Save(Side),
    Close,
    /// 保存していない変更があるまま閉じるか（`true` なら閉じる）
    ConfirmClose(bool),
    /// 裏の糸で比べ終わった（頼んだ番号と結果）
    Computed(u64, Box<Diff>),
    /// 次の違い・前の違いへ（R-02 a。F7 / Shift+F7）
    Next,
    Previous,
    /// 選んでいる違いを、もう片方から `Side` の面へ写す（R-02 c）
    CopyTo(Side),
    /// 左右を入れ替える（D-4）
    Swap,
    /// 差分の地図を押した（R-02 e。左右に並べたときの段）
    MapPick(usize),
    /// 1 列の面の編集部品からの通知（読むだけ。D-7）
    Unified(Action),
    /// 差分からパッチを作る（R-03）。保存先を聞く
    MakePatch,
    PickedPatchPath(Option<PathBuf>),
    /// パッチを開いて編集中の文書に当てる（R-04）
    OpenPatch,
    PickedPatchFile(Option<PathBuf>),
    LoadedPatch(Result<Box<LoadedFile>, String>),
    /// 当てるファイルの部分を選んだ（D-6）。`None` なら取り消し
    ChoosePatch(Option<usize>),
    /// プレビューでの比較の面からの通知（v2.6.0 R-04 b）
    Preview(Side, crate::render::PreviewAction),
}

/// 何と何を比べているか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// ファイルとファイル（左右とも保存できる）
    Files,
    /// 保存してある元（左）と編集中の文書（右）。**左は保存できない**（D-8）
    Saved,
}

/// 面が持つ文書（「保存してある元と比べる」の右の面は持たない。編集中の文書を使う）。
pub struct PaneDoc {
    pub document: Document,
    pub editor: EditorState,
    pub history: History,
    pub meta: DocumentMeta,
}

pub struct Pane {
    /// `None` なら編集中の文書（`App::document`）
    pub doc: Option<PaneDoc>,
    pub title: String,
    /// 保存できるか（D-8。保存してある元は保存できない）。**左右を入れ替えても面に付いて回る**
    pub saveable: bool,
}

impl Pane {
    fn own(file: LoadedFile, title: String) -> Self {
        let meta = DocumentMeta::opened(file.path, file.format);
        Self {
            doc: Some(PaneDoc {
                document: Document::from_text(file.text),
                editor: EditorState::default(),
                history: History::new(),
                meta,
            }),
            title,
            saveable: true,
        }
    }
}

pub struct DiffScreen {
    pub kind: Kind,
    pub left: Pane,
    pub right: Pane,
    pub options: Options,
    pub diff: Diff,
    /// 左右の空きの段の表
    pub tables: [Wrap; 2],
    /// 左右に塗る印
    pub marks: [DiffMarks; 2],
    /// 打鍵を受ける面
    pub focus: Side,
    /// 比べ直しを頼んだ番号。**古い答えは捨てる**
    pub generation: u64,
    /// 裏の糸で比べている最中か
    pub computing: bool,
    /// 比べたあとに書き換えたか（比べ直しが要る）
    pub stale: bool,
    /// 閉じる前の確認を出しているか
    pub confirm_close: bool,
    /// 選んでいる違い（`Diff::changes` の何番目か）。移動・写す・枠の対象
    pub current: Option<usize>,
    /// 差分の地図の印（R-02 e）
    pub map: Vec<crate::render::diffmap::MapMark>,
    /// 出し方（設定の `diff_layout`。**作り直したときの出し方を覚える**）
    pub layout: crate::io::settings::DiffLayout,
    /// 1 列に混ぜた面（読むだけ。D-7）。1 列で出しているときだけ作る
    pub unified: Option<PaneDoc>,
    pub unified_marks: DiffMarks,
    /// 違いごとの、1 列の面での最初の行
    pub unified_starts: Vec<usize>,
    /// プレビューでの比較（v2.6.0 R-04 b）。左右の送り具合と、違うブロックの色
    pub previews: [crate::render::PreviewState; 2],
    pub preview_marks: [Vec<(usize, Color)>; 2],
}

fn index(side: Side) -> usize {
    match side {
        Side::Left => 0,
        Side::Right => 1,
    }
}

impl DiffScreen {
    fn new(kind: Kind, left: Pane, right: Pane, options: Options) -> Self {
        Self {
            kind,
            left,
            right,
            options,
            diff: Diff::default(),
            tables: [Wrap::spacers(0, &[]), Wrap::spacers(0, &[])],
            marks: [DiffMarks::default(), DiffMarks::default()],
            focus: Side::Right,
            generation: 0,
            computing: false,
            stale: true,
            confirm_close: false,
            current: None,
            map: Vec::new(),
            layout: crate::io::settings::DiffLayout::SideBySide,
            unified: None,
            unified_marks: DiffMarks::default(),
            unified_starts: Vec::new(),
            previews: Default::default(),
            preview_marks: [Vec::new(), Vec::new()],
        }
    }

    /// 1 列で出しているか
    pub fn is_unified(&self) -> bool {
        self.layout == crate::io::settings::DiffLayout::Unified
    }

    /// プレビューで出しているか（v2.6.0 R-04 b）
    pub fn is_preview(&self) -> bool {
        self.layout == crate::io::settings::DiffLayout::Preview
    }

    pub fn pane(&self, side: Side) -> &Pane {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn pane_mut(&mut self, side: Side) -> &mut Pane {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// 面を保存できるか（D-8。保存してある元は保存できない）
    pub fn can_save(&self, side: Side) -> bool {
        self.pane(side).saveable
    }

    /// `n` 番目の違い
    fn change(&self, n: usize) -> Option<&Block> {
        self.diff.changes().nth(n)
    }

    /// 選んでいる違いを枠で示す
    fn mark_current(&mut self) {
        let block = self.current.and_then(|n| self.change(n)).cloned();
        self.marks[0].current = block.as_ref().map(|block| block.left().clone());
        self.marks[1].current = block.as_ref().map(|block| block.right().clone());
    }

    /// 保存していない変更のある面（自分で持つ文書だけ。編集中の文書は閉じても残る）
    fn unsaved(&self) -> bool {
        [&self.left, &self.right]
            .iter()
            .any(|pane| pane.doc.as_ref().is_some_and(|doc| doc.meta.dirty))
    }
}

/// diff の画面を出している間、メッセージをどう扱うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// 打鍵を受けている面へ回す
    Pane,
    /// 打鍵を受けている面を保存する
    Save,
    /// 受け付けない（編集中の文書を差し替える・本文の外の画面を出す操作）
    Blocked,
    /// ふだんどおり
    Pass,
}

/// メッセージの扱いを決める。
pub(super) fn route(message: &Message) -> Route {
    match message {
        Message::Editor(_)
        | Message::Cut
        | Message::Copy
        | Message::Paste
        | Message::Pasted(_)
        | Message::SelectAll
        | Message::AddCaret(_)
        | Message::SelectOccurrence(_)
        | Message::Transform(_)
        | Message::Indent(_)
        | Message::DuplicateLine
        | Message::DeleteLine
        | Message::JoinLines
        | Message::InsertStamp(_)
        | Message::MatchBracket
        | Message::Undo
        | Message::Redo
        | Message::ToggleComment { .. }
        | Message::Format(_)
        | Message::FormatTable
        | Message::TableAddRow
        | Message::TableAddColumn
        | Message::TableEdit(_)
        | Message::ClosingBracket
        | Message::Toc(_)
        | Message::ClipboardImage(_)
        // 検索・置き換え（v2.8.0 R-02）は、いま入力のある面で
        | Message::OpenSearch
        | Message::FocusSearch
        | Message::SearchInput(_)
        | Message::SearchSubmit
        | Message::SearchStep(_)
        | Message::ToggleRegex
        | Message::ToggleCase
        | Message::ToggleWholeWord
        | Message::ReplaceOne
        | Message::ReplaceAll
        | Message::SearchFound { .. } => Route::Pane,
        Message::File(FileCommand::Save) => Route::Save,
        Message::File(_)
        | Message::OpenBrowser
        | Message::OpenRecent(_)
        | Message::FileDropped(_)
        | Message::ReopenAs(_)
        | Message::SaveWithEncoding(..)
        | Message::SaveWithLineEnding(_)
        | Message::OpenGoto
        | Message::SetMode(_)
        | Message::ToggleToc
        | Message::JumpTo(_)
        | Message::Fold
        | Message::Unfold
        | Message::FoldAll
        | Message::UnfoldAll
        | Message::OpenHeadingPicker
        | Message::HeadingStep(_)
        | Message::CheckLinks
        | Message::OpenLinkAtCaret
        | Message::Seek(_)
        | Message::OpenEncodingDialog
        | Message::ReloadExternal
        | Message::Lint(_)
        | Message::ToggleReadOnly => Route::Blocked,
        _ => Route::Pass,
    }
}

/// 面の題名。**左右で名前が同じなら、親のフォルダも出す**
fn titles(left: &std::path::Path, right: &std::path::Path) -> (String, String) {
    let name = |path: &std::path::Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    };
    if name(left) != name(right) {
        return (name(left), name(right));
    }
    let with_parent = |path: &std::path::Path| match path.parent().and_then(|p| p.file_name()) {
        Some(parent) => format!("{}/{}", parent.to_string_lossy(), name(path)),
        None => name(path),
    };
    (with_parent(left), with_parent(right))
}

/// 片側の面に塗る印を作る。
fn marks_for(diff: &Diff, side: Side, lines: usize, inline: &InlineMap) -> DiffMarks {
    let mut marks = DiffMarks {
        lines: vec![LineMark::Same; lines],
        inline: std::collections::HashMap::new(),
        changed: CHANGED,
        only: match side {
            Side::Left => REMOVED,
            Side::Right => ADDED,
        },
        other: ADDED,
        inline_color: INLINE,
        spacer: SPACER,
        current: None,
    };
    for block in diff.changes() {
        let (mine, theirs) = match side {
            Side::Left => (block.left(), block.right()),
            Side::Right => (block.right(), block.left()),
        };
        let mark = if theirs.is_empty() {
            LineMark::Only
        } else {
            LineMark::Changed
        };
        for line in mine.clone() {
            if let Some(entry) = marks.lines.get_mut(line) {
                *entry = mark;
            }
        }
    }
    for (&(left, right), (ours, theirs)) in inline {
        let (line, ranges) = match side {
            Side::Left => (left, ours),
            Side::Right => (right, theirs),
        };
        if !ranges.is_empty() {
            marks.inline.insert(line, ranges.clone());
        }
    }
    marks
}

/// 変わった行の組（左の行・右の行）ごとの、行の中の違い。
type InlineMap = std::collections::HashMap<
    (usize, usize),
    (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>),
>;

/// 1 列の面の行の頭（元の行番号 2 つと印）。**幅を揃える**（行の中の違いの位置をずらすため、長さを決めておく）
const UNIFIED_PREFIX: usize = 14;

fn unified_prefix(left: Option<usize>, right: Option<usize>, sign: char) -> String {
    let number = |n: Option<usize>| n.map_or(String::new(), |n| (n + 1).to_string());
    format!("{:>5} {:>5} {sign} ", number(left), number(right))
}

/// 1 列に混ぜた文書と、その印・違いごとの最初の行を作る（R-02。読むだけ。D-7）
fn build_unified(
    diff: &Diff,
    left: &Document,
    right: &Document,
    inline: &InlineMap,
) -> (PaneDoc, DiffMarks, Vec<usize>) {
    use crate::diff::UnifiedRow;
    let mut by_left: std::collections::HashMap<usize, &Vec<std::ops::Range<usize>>> =
        std::collections::HashMap::new();
    let mut by_right: std::collections::HashMap<usize, &Vec<std::ops::Range<usize>>> =
        std::collections::HashMap::new();
    for (&(l, r), (ours, theirs)) in inline {
        by_left.insert(l, ours);
        by_right.insert(r, theirs);
    }
    let rows = diff.unified_rows();
    let mut text = String::new();
    let mut marks = DiffMarks {
        lines: Vec::with_capacity(rows.len()),
        inline: std::collections::HashMap::new(),
        changed: CHANGED,
        only: REMOVED,
        other: ADDED,
        inline_color: INLINE,
        spacer: SPACER,
        current: None,
    };
    let shift = |ranges: &Vec<std::ops::Range<usize>>| {
        ranges
            .iter()
            .map(|range| range.start + UNIFIED_PREFIX..range.end + UNIFIED_PREFIX)
            .collect::<Vec<_>>()
    };
    let mut starts = Vec::new();
    let mut in_change = false;
    for (index, row) in rows.iter().enumerate() {
        let (prefix, body, mark) = match *row {
            UnifiedRow::Same(l, r) => (
                unified_prefix(Some(l), Some(r), ' '),
                line_text(left, l),
                LineMark::Same,
            ),
            UnifiedRow::Removed(l) => (
                unified_prefix(Some(l), None, '-'),
                line_text(left, l),
                LineMark::Only,
            ),
            UnifiedRow::Added(r) => (
                unified_prefix(None, Some(r), '+'),
                line_text(right, r),
                LineMark::Other,
            ),
        };
        let changed = mark != LineMark::Same;
        if changed && !in_change {
            starts.push(index);
        }
        in_change = changed;
        match *row {
            UnifiedRow::Removed(l) => {
                if let Some(ranges) = by_left.get(&l) {
                    marks.inline.insert(index, shift(ranges));
                }
            }
            UnifiedRow::Added(r) => {
                if let Some(ranges) = by_right.get(&r) {
                    marks.inline.insert(index, shift(ranges));
                }
            }
            UnifiedRow::Same(..) => {}
        }
        marks.lines.push(mark);
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&prefix);
        text.push_str(body.trim_end_matches(['\n', '\r']));
    }
    let doc = PaneDoc {
        document: Document::from_text(text),
        editor: EditorState::default(),
        history: History::new(),
        meta: DocumentMeta::untitled(),
    };
    (doc, marks, starts)
}

/// 2 つの文字列で違う範囲（バイト位置）。**同じ頭と同じ尻を除く**（文字の境目で切る）
pub(super) fn changed_span(old: &str, new: &str) -> (usize, usize, usize) {
    let mut start = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let mut tail = old[start..]
        .bytes()
        .rev()
        .zip(new[start..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - tail) || !new.is_char_boundary(new.len() - tail) {
        tail -= 1;
    }
    (start, old.len() - tail, new.len() - tail)
}

/// 行の頭のバイト位置。**最後の行の次は文書の末尾**
fn line_byte(rope: &ropey::Rope, line: usize) -> usize {
    if line >= rope.len_lines() {
        rope.len_bytes()
    } else {
        rope.line_to_byte(line)
    }
}

/// 行を含むブロック（行の前にブロックが無ければ最初のもの）
fn block_at_line(document: &Document, line: usize) -> usize {
    document
        .blocks()
        .partition_point(|block| block.start_line <= line)
        .saturating_sub(1)
}

/// プレビューでの比較の色（v2.6.0 R-04 b）。**違いに掛かるブロック**に、消えた・足した・変わったの色を付ける
fn preview_marks(document: &Document, diff: &Diff, side: Side) -> Vec<(usize, Color)> {
    let mut out = Vec::new();
    for (index, block) in document.blocks().iter().enumerate() {
        let lines = block.start_line..block.start_line + block.line_count.max(1);
        let found = diff.changes().find_map(|change| {
            let (own, other) = match side {
                Side::Left => (change.left(), change.right()),
                Side::Right => (change.right(), change.left()),
            };
            (own.start < lines.end && lines.start < own.end).then_some(other.is_empty())
        });
        if let Some(only_here) = found {
            let color = match (only_here, side) {
                (true, Side::Left) => REMOVED,
                (true, Side::Right) => ADDED,
                (false, _) => CHANGED,
            };
            out.push((index, color));
        }
    }
    out
}

fn line_text(document: &Document, line: usize) -> String {
    let rope = document.text();
    if line >= rope.len_lines() {
        return String::new();
    }
    rope.line(line).to_string()
}

impl App {
    /// diff の画面へのメッセージを処理する。
    pub(super) fn diff_message(&mut self, message: DiffMessage) -> Task<Message> {
        match message {
            DiffMessage::OpenFiles { in_app } => {
                if self.picking || self.diff_screen.is_some() {
                    return Task::none();
                }
                self.notice = Some(super::notice::Notice::plain(
                    "比べる 1 つ目（左）のファイルを選んでください".to_owned(),
                ));
                return self
                    .ask_for_compare(in_app)
                    .map(move |path| Message::Diff(DiffMessage::PickedLeft { path, in_app }));
            }
            DiffMessage::PickedLeft { path, in_app } => {
                self.picking = false;
                self.browser_choice = None;
                let Some(left) = path else {
                    self.notice = None;
                    return Task::none();
                };
                self.notice = Some(super::notice::Notice::plain(
                    "比べる 2 つ目（右）のファイルを選んでください".to_owned(),
                ));
                return self.ask_for_compare(in_app).map(move |path| {
                    Message::Diff(DiffMessage::PickedRight {
                        left: left.clone(),
                        path,
                    })
                });
            }
            DiffMessage::PickedRight { left, path } => {
                self.picking = false;
                self.browser_choice = None;
                let Some(right) = path else {
                    self.notice = None;
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        let load = |path: &std::path::Path| {
                            crate::io::load_as(path, None)
                                .map(Box::new)
                                .map_err(|error| format!("{}: {error}", path.display()))
                        };
                        Ok((load(&left)?, load(&right)?))
                    },
                    |result| Message::Diff(DiffMessage::LoadedFiles(result)),
                );
            }
            DiffMessage::LoadedFiles(result) => match result {
                Ok((left, right)) => {
                    self.notice = lossy_notice(&left).or_else(|| lossy_notice(&right));
                    let (left_title, right_title) = titles(&left.path, &right.path);
                    let mut screen = DiffScreen::new(
                        Kind::Files,
                        Pane::own(*left, left_title),
                        Pane::own(*right, right_title),
                        self.diff_options(),
                    );
                    screen.layout = self.settings.diff_layout;
                    self.diff_screen = Some(screen);
                    return self.diff_recompute();
                }
                Err(error) => self.notice = Some(super::notice::Notice::plain(error)),
            },
            DiffMessage::OpenSaved => {
                if self.diff_screen.is_some() {
                    return Task::none();
                }
                let Some(path) = self.meta.path.clone() else {
                    self.notice = Some(super::notice::Notice::plain(
                        "まだ保存していない文書は、保存してある元と比べられません".to_owned(),
                    ));
                    return Task::none();
                };
                // **編集中の文書と同じ文字コードで読む**（違う文字コードで読むと全行が違って見える）
                let encoding = self.meta.format.encoding;
                return Task::perform(
                    async move {
                        crate::io::load_as(&path, Some(encoding))
                            .map(Box::new)
                            .map_err(|error| error.to_string())
                    },
                    |result| Message::Diff(DiffMessage::LoadedSaved(result)),
                );
            }
            DiffMessage::LoadedSaved(result) => match result {
                Ok(file) => {
                    self.notice = lossy_notice(&file);
                    let name = file
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let mut left = Pane::own(*file, format!("{name}（保存してある元）"));
                    left.saveable = false;
                    let right = Pane {
                        doc: None,
                        title: format!("{name}（編集中）"),
                        saveable: true,
                    };
                    let mut screen = DiffScreen::new(Kind::Saved, left, right, self.diff_options());
                    screen.layout = self.settings.diff_layout;
                    self.diff_screen = Some(screen);
                    return self.diff_recompute();
                }
                Err(error) => self.notice = Some(super::notice::Notice::plain(error)),
            },
            DiffMessage::Pane(side, action) => return self.pane_action(side, action),
            DiffMessage::Save(side) => return self.save_pane(side),
            DiffMessage::Close => {
                let Some(screen) = self.diff_screen.as_mut() else {
                    return Task::none();
                };
                if screen.unsaved() && !screen.confirm_close {
                    screen.confirm_close = true;
                    return Task::none();
                }
                self.close_diff();
            }
            DiffMessage::ConfirmClose(close) => {
                if close {
                    self.close_diff();
                } else if let Some(screen) = self.diff_screen.as_mut() {
                    screen.confirm_close = false;
                }
            }
            DiffMessage::Next => self.step_change(true),
            DiffMessage::Previous => self.step_change(false),
            DiffMessage::CopyTo(target) => return self.copy_current(target),
            DiffMessage::Swap => return self.swap_panes(),
            DiffMessage::MapPick(row) => self.scroll_to_row(row),
            DiffMessage::Unified(action) => return self.unified_action(action),
            DiffMessage::MakePatch => return self.ask_patch_path(),
            DiffMessage::PickedPatchPath(path) => {
                self.picking = false;
                self.browser_choice = None;
                if let Some(path) = path {
                    self.write_patch(&path);
                }
            }
            DiffMessage::OpenPatch => {
                if self.picking || self.diff_screen.is_some() {
                    return Task::none();
                }
                let picker = super::picker::Picker::open_kind("パッチ", &["patch", "diff"]);
                return self
                    .ask(picker)
                    .map(|path| Message::Diff(DiffMessage::PickedPatchFile(path)));
            }
            DiffMessage::PickedPatchFile(path) => {
                self.picking = false;
                self.browser_choice = None;
                let Some(path) = path else {
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        crate::io::load(&path)
                            .map(Box::new)
                            .map_err(|error| error.to_string())
                    },
                    |result| Message::Diff(DiffMessage::LoadedPatch(result)),
                );
            }
            DiffMessage::LoadedPatch(result) => match result {
                Ok(file) => self.choose_patch(&file.text),
                Err(error) => self.notice = Some(super::notice::Notice::plain(error)),
            },
            DiffMessage::ChoosePatch(choice) => {
                let files = self.patch_choice.take().unwrap_or_default();
                if let Some(file) = choice.and_then(|index| files.get(index)) {
                    self.apply_patch(file);
                }
            }
            DiffMessage::Preview(side, action) => self.diff_preview_action(side, action),
            DiffMessage::Computed(generation, diff) => {
                let Some(screen) = self.diff_screen.as_mut() else {
                    return Task::none();
                };
                if screen.generation != generation {
                    return Task::none();
                }
                screen.computing = false;
                self.diff_apply(*diff);
                if self.diff_screen.as_ref().is_some_and(|screen| screen.stale) {
                    return self.diff_recompute();
                }
            }
        }
        Task::none()
    }

    /// 比べるファイルを選ぶ。`in_app` ならアプリ内で選ぶ（OS のダイアログが使えない環境の退避路と同じ）
    fn ask_for_compare(&mut self, in_app: bool) -> Task<Option<PathBuf>> {
        let picker = super::picker::Picker::open();
        if in_app {
            self.picking = true;
            let browser = super::browser_from(&picker, &self.meta);
            return self.ask_in_app(browser);
        }
        self.ask(picker)
    }

    /// 設定の比べ方（R-02 d）
    fn diff_options(&self) -> Options {
        use crate::io::settings::DiffWhitespace;
        Options {
            whitespace: match self.settings.diff_whitespace {
                DiffWhitespace::Exact => crate::diff::Whitespace::Exact,
                DiffWhitespace::IgnoreChanges => crate::diff::Whitespace::IgnoreChanges,
                DiffWhitespace::IgnoreAll => crate::diff::Whitespace::IgnoreAll,
            },
            ignore_case: self.settings.diff_ignore_case,
            ignore_eol: self.settings.diff_ignore_eol,
        }
    }

    /// 設定の比べ方を、出している画面へ移して比べ直す（設定画面と、画面の帯の切り替えから）
    pub(super) fn sync_diff_options(&mut self) -> Task<Message> {
        let options = self.diff_options();
        let layout = self.settings.diff_layout;
        let Some(screen) = self.diff_screen.as_mut() else {
            return Task::none();
        };
        // 出し方だけ変わったなら、比べ直さずに面を作り直す
        if screen.layout != layout {
            screen.layout = layout;
            if screen.options == options {
                let diff = screen.diff.clone();
                let current = screen.current;
                self.diff_apply(diff);
                // **選んでいた違いの場所で出す**（切り替えても読んでいた所を見失わない）
                if let Some(n) = current {
                    self.goto_change(n);
                }
                return Task::none();
            }
        }
        if screen.options == options {
            return Task::none();
        }
        screen.options = options;
        screen.stale = true;
        self.diff_recompute()
    }

    /// 閉じる。**編集中の文書の印と点滅を戻す**
    fn close_diff(&mut self) {
        self.diff_screen = None;
        self.notice = None;
        self.editor.caret_visible = true;
        // フォルダーの比較から開いていたら、その行を比べ直す（v2.6.0 R-04 a）
        self.folder_reopen();
    }

    /// 打鍵を受けている面へメッセージを回す（`update` の入口から呼ぶ）。
    pub(super) fn route_to_pane(&mut self, message: Message) -> Task<Message> {
        if self
            .diff_screen
            .as_ref()
            .is_some_and(DiffScreen::is_unified)
        {
            return self.in_unified(|app| app.handle(message));
        }
        let Some(side) = self.diff_screen.as_ref().map(|screen| screen.focus) else {
            return Task::none();
        };
        let task = self.in_pane(side, |app| app.handle(message));
        let after = self.after_pane(side);
        Task::batch([task, after])
    }

    /// 面の編集部品からの通知。**触った面に打鍵を移す**（巻き上げだけでは移さない）
    fn pane_action(&mut self, side: Side, action: Action) -> Task<Message> {
        let touched = !matches!(
            action,
            Action::Scrolled { .. }
                | Action::ScrolledX { .. }
                | Action::ScrollXTo { .. }
                | Action::ScrollTo { .. }
                | Action::ScrollToRow { .. }
                | Action::WrapColumns(_)
        );
        if touched {
            if let Some(screen) = self.diff_screen.as_mut() {
                screen.focus = side;
            }
            self.diff_blink_reset();
        }
        let task = self.in_pane(side, |app| app.handle(Message::Editor(action)));
        // **キャレットのある違いを選ぶ**（押したところの違いを写せるように）
        if touched {
            self.select_change_at_caret(side);
        }
        let after = self.after_pane(side);
        Task::batch([task, after])
    }

    /// キャレットが違いの中にあれば、それを選ぶ
    fn select_change_at_caret(&mut self, side: Side) {
        let Some(mut screen) = self.diff_screen.take() else {
            return;
        };
        let line = pane_editor(&screen, side, &self.editor).cursor_line;
        if let Some(block) = screen.diff.block_of(side, line) {
            if screen.diff.blocks()[block].is_change() {
                let n = screen.diff.blocks()[..block]
                    .iter()
                    .filter(|block| block.is_change())
                    .count();
                screen.current = Some(n);
                screen.mark_current();
            }
        }
        self.diff_screen = Some(screen);
    }

    /// 次の違い・前の違いへ移る（R-02 a）。**端では止まる**（回り込まない）
    fn step_change(&mut self, forward: bool) {
        let Some(screen) = self.diff_screen.as_ref() else {
            return;
        };
        let count = screen.diff.change_count();
        if count == 0 {
            return;
        }
        let next = match (screen.current, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(n), true) => (n + 1).min(count - 1),
            (Some(n), false) => n.saturating_sub(1),
        };
        self.goto_change(next);
    }

    /// `n` 番目の違いへ移る。**左右とも違いの頭にキャレットを置き、上に 3 段の余白を残して見せる**
    fn goto_change(&mut self, n: usize) {
        let Some(mut screen) = self.diff_screen.take() else {
            return;
        };
        if screen.is_unified() {
            if let (Some(&line), Some(doc)) =
                (screen.unified_starts.get(n), screen.unified.as_mut())
            {
                screen.current = Some(n);
                doc.editor.place_caret(line, 0);
                doc.editor.top_line = line.saturating_sub(3);
                doc.editor.top_segment = (doc.editor.top_line, 0);
            }
            self.diff_screen = Some(screen);
            return;
        }
        let Some(block) = screen.change(n).cloned() else {
            self.diff_screen = Some(screen);
            return;
        };
        screen.current = Some(n);
        screen.mark_current();
        // 左右の段は揃っているので、どちらの表で数えても同じ段になる
        let row = screen.tables[0].row_of(block.left().start, 0);
        let top = row.saturating_sub(3);
        for side in [Side::Left, Side::Right] {
            let range = match side {
                Side::Left => block.left().clone(),
                Side::Right => block.right().clone(),
            };
            let (top_line, segment) = screen.tables[index(side)].position(top);
            let lines = match &screen.pane(side).doc {
                Some(doc) => doc.document.text().len_lines(),
                None => self.document.text().len_lines(),
            };
            let editor = match screen.pane_mut(side).doc.as_mut() {
                Some(doc) => &mut doc.editor,
                None => &mut self.editor,
            };
            let line = range.start.min(lines.saturating_sub(1));
            editor.place_caret(line, 0);
            editor.top_line = top_line;
            editor.top_segment = (top_line, segment);
            // プレビューでの比較も、違いのあるブロックへ送る（v2.6.0 R-04 b）
            let document = pane_document(&screen, side, &self.document);
            let block = block_at_line(document, line);
            screen.previews[index(side)].anchor = crate::layout::ScrollAnchor {
                block_id: block,
                offset_in_block: 0.0,
            };
        }
        self.diff_screen = Some(screen);
    }

    /// プレビューでの比較の面からの通知（v2.6.0 R-04 b）。送ったら、もう片方を対応するブロックへ揃える
    fn diff_preview_action(&mut self, side: Side, action: crate::render::PreviewAction) {
        use crate::render::PreviewAction;
        let Some(mut screen) = self.diff_screen.take() else {
            return;
        };
        match action {
            PreviewAction::Scrolled(delta) => {
                let viewport = 600.0;
                let own = pane_document(&screen, side, &self.document);
                let anchor =
                    screen.previews[index(side)]
                        .anchor
                        .scrolled(delta, own.heights(), viewport);
                // もう片方は、見えている頭のブロックの行に対応する行のブロックへ
                let other = match side {
                    Side::Left => Side::Right,
                    Side::Right => Side::Left,
                };
                let line = own
                    .blocks()
                    .get(anchor.block_id)
                    .map_or(0, |block| block.start_line);
                let row = screen.diff.row_of(side, line);
                let other_line = screen.diff.line_at_row(other, row);
                let other_doc = pane_document(&screen, other, &self.document);
                let block = block_at_line(other_doc, other_line);
                // ブロックの中の位置は、高さの比で写す
                let own_height = own.heights().height(anchor.block_id);
                let other_height = other_doc.heights().height(block);
                let other_anchor = crate::layout::ScrollAnchor {
                    block_id: block,
                    offset_in_block: if own_height > 0.0 {
                        anchor.offset_in_block / own_height * other_height
                    } else {
                        0.0
                    },
                };
                screen.previews[index(side)].anchor = anchor;
                screen.previews[index(other)].anchor = other_anchor;
            }
            PreviewAction::Measured {
                updates,
                embed_requests,
                ..
            } => {
                // 面の文書の高さを直し、図を頼む（**ほかの面の図を捨てない**よう、まとめて渡さず 1 つずつ）
                match screen.pane_mut(side).doc.as_mut() {
                    Some(doc) => doc.document.set_heights(&updates),
                    None => self.document.set_heights(&updates),
                }
                for request in embed_requests {
                    self.embeds.request(request);
                }
                self.preview.embeds_pending = self.embeds.pending();
            }
            _ => {}
        }
        self.diff_screen = Some(screen);
    }

    /// 選んでいる違いを、もう片方から `target` の面へ写す（R-02 c）。
    ///
    /// **行をまるごと置き換える。** 写した面は未保存になり、元に戻す（Ctrl+Z）で 1 回で戻せる
    fn copy_current(&mut self, target: Side) -> Task<Message> {
        let Some(screen) = self.diff_screen.as_ref() else {
            return Task::none();
        };
        let Some(block) = screen.current.and_then(|n| screen.change(n)).cloned() else {
            return Task::none();
        };
        let (source_range, target_range) = match target {
            Side::Left => (block.right().clone(), block.left().clone()),
            Side::Right => (block.left().clone(), block.right().clone()),
        };
        let source = match target {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        let text = {
            let rope = pane_document(screen, source, &self.document).text();
            rope.byte_slice(line_byte(rope, source_range.start)..line_byte(rope, source_range.end))
                .to_string()
        };
        self.in_pane(target, |app| {
            let range = {
                let rope = app.document.text();
                line_byte(rope, target_range.start)..line_byte(rope, target_range.end)
            };
            let removed = app.document.text().byte_slice(range.clone()).to_string();
            let at = range.start;
            app.replace_range(range, removed, text);
            // 写したところを選んだままにしない（次の違いへ移って続けて写せるように）
            app.move_caret_to_byte(at);
        });
        if let Some(screen) = self.diff_screen.as_mut() {
            screen.focus = target;
        }
        // 比べ直すと、写した違いは消える。**番号はそのまま**（次の違いを指す）
        self.after_pane(target)
    }

    /// 面に見えている段の数（おおよそ。**窓の高さから帯と見出しの分を引いて割る**）
    fn visible_rows(&self) -> usize {
        let (_, line_height) = self.editor_sizes();
        let height = self.window_size.map_or(800.0, |size| size.height) - 170.0;
        ((height / line_height.max(1.0)).floor().max(1.0)) as usize
    }

    /// 押した段が真ん中に来るよう、左右の面を巻き上げる（R-02 e）
    fn scroll_to_row(&mut self, row: usize) {
        let visible = self.visible_rows();
        let Some(mut screen) = self.diff_screen.take() else {
            return;
        };
        let top = row.saturating_sub(visible / 2);
        for side in [Side::Left, Side::Right] {
            let (line, segment) = screen.tables[index(side)].position(top);
            let editor = match screen.pane_mut(side).doc.as_mut() {
                Some(doc) => &mut doc.editor,
                None => &mut self.editor,
            };
            editor.top_line = line;
            editor.top_segment = (line, segment);
        }
        self.diff_screen = Some(screen);
    }

    /// 左右を入れ替える（D-4）。パッチの元（左）と新（右）も入れ替わる
    fn swap_panes(&mut self) -> Task<Message> {
        let Some(screen) = self.diff_screen.as_mut() else {
            return Task::none();
        };
        std::mem::swap(&mut screen.left, &mut screen.right);
        screen.focus = match screen.focus {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        screen.stale = true;
        self.diff_recompute()
    }

    /// 面の文書を本文として扱いながら `f` を呼ぶ。
    ///
    /// **入れ替えるのは、本文・キャレット・履歴・素性・空きの段の表・畳み込み**。
    /// 編集中の文書を使う面（保存してある元と比べるときの右）では、表と畳み込みだけを入れ替える
    pub(super) fn in_pane<R>(&mut self, side: Side, f: impl FnOnce(&mut App) -> R) -> R {
        let Some(mut screen) = self.diff_screen.take() else {
            return f(self);
        };
        let i = index(side);
        // 空きの段の表を折り返しの表の場所へ入れる（上下の移動とホイールが段で動く）
        let table = std::mem::replace(&mut screen.tables[i], Wrap::spacers(0, &[]));
        let main_wrap = self.wrap.replace(table);
        // **diff の面は畳まない**（左右の段の対応が崩れる）
        let main_folds = std::mem::take(&mut self.folds);
        let main_headings = std::mem::take(&mut self.fold_headings);

        let before;
        let changed;
        let result = match screen.pane_mut(side).doc.as_mut() {
            Some(doc) => {
                std::mem::swap(&mut self.document, &mut doc.document);
                std::mem::swap(&mut self.editor, &mut doc.editor);
                std::mem::swap(&mut self.history, &mut doc.history);
                std::mem::swap(&mut self.meta, &mut doc.meta);
                // 面の文書は読み取り専用にしない（編集中の文書の設定とは別）
                let read_only = std::mem::replace(&mut self.read_only, false);
                self.diff_own = true;
                before = self.document.version();
                let result = f(self);
                changed = self.document.version() != before;
                self.diff_own = false;
                self.read_only = read_only;
                std::mem::swap(&mut self.document, &mut doc.document);
                std::mem::swap(&mut self.editor, &mut doc.editor);
                std::mem::swap(&mut self.history, &mut doc.history);
                std::mem::swap(&mut self.meta, &mut doc.meta);
                result
            }
            None => {
                before = self.document.version();
                let result = f(self);
                changed = self.document.version() != before;
                result
            }
        };

        let table = std::mem::replace(&mut self.wrap, main_wrap);
        screen.tables[i] = table.unwrap_or_else(|| Wrap::spacers(0, &[]));
        self.folds = main_folds;
        self.fold_headings = main_headings;
        let main_pane = screen.pane(side).doc.is_none();
        if changed {
            screen.stale = true;
        }
        self.diff_screen = Some(screen);
        // 編集中の文書を書き換えたら、畳み込みを作り直す（入れ替えている間に作ったものは捨てた）
        if changed && main_pane {
            self.rebuild_folds();
        }
        result
    }

    /// パッチの保存先を聞く（R-03）。**左の文書の置き場と名前から始める**
    fn ask_patch_path(&mut self) -> Task<Message> {
        if self.picking {
            return Task::none();
        }
        let Some(screen) = self.diff_screen.as_ref() else {
            return Task::none();
        };
        if screen.diff.is_identical() {
            self.notice = Some(super::notice::Notice::plain(
                "違いが無いので、パッチは作れません".to_owned(),
            ));
            return Task::none();
        }
        let path = pane_meta(screen, Side::Left, &self.meta).path.clone();
        let stem = path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "changes".to_owned());
        let directory = path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from));
        let picker = super::picker::Picker::save("パッチ", "patch", format!("{stem}.patch"))
            .in_directory(directory);
        self.ask(picker)
            .map(|path| Message::Diff(DiffMessage::PickedPatchPath(path)))
    }

    /// パッチを書き出す（R-03）。**左を元、右を新**（D-4）。改行コードは左の文書に合わせる
    fn write_patch(&mut self, path: &std::path::Path) {
        let Some(screen) = self.diff_screen.as_ref() else {
            return;
        };
        let header = |side: Side| {
            let meta = pane_meta(screen, side, &self.meta);
            let name = meta
                .path
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| screen.pane(side).title.clone());
            // **保存したままなら、ファイルの更新日時**（GNU diff と同じ）。直していれば今の日時
            let modified = (!meta.dirty)
                .then(|| {
                    meta.path
                        .as_ref()
                        .and_then(|path| std::fs::metadata(path).ok())
                })
                .flatten()
                .and_then(|metadata| metadata.modified().ok())
                .map(chrono::DateTime::<chrono::Local>::from)
                .unwrap_or_else(chrono::Local::now);
            format!("{name}\t{}", modified.format("%Y-%m-%d %H:%M:%S%.9f %z"))
        };
        let (old_header, new_header) = (header(Side::Left), header(Side::Right));
        let old = pane_document(screen, Side::Left, &self.document)
            .text()
            .to_string();
        let new = pane_document(screen, Side::Right, &self.document)
            .text()
            .to_string();
        let format = match self.settings.patch_format {
            crate::io::settings::PatchFormat::Unified => crate::diff::patch::Format::Unified,
            crate::io::settings::PatchFormat::Context => crate::diff::patch::Format::Context,
        };
        let mut patch = crate::diff::patch::write(&old, &new, &old_header, &new_header, format);
        if pane_meta(screen, Side::Left, &self.meta).format.line_ending
            == crate::io::LineEnding::Crlf
        {
            patch = patch.replace('\n', "\r\n");
        }
        self.notice = Some(super::notice::Notice::plain(
            match std::fs::write(path, patch) {
                Ok(()) => format!("パッチを書き出しました: {}", path.display()),
                Err(error) => format!("パッチを書き出せません: {error}"),
            },
        ));
    }

    /// 読んだパッチの中から、当てるファイルの部分を決める（D-6）
    pub(super) fn choose_patch(&mut self, text: &str) {
        let files = match crate::diff::patch::parse(text) {
            Ok(files) => files,
            Err(error) => {
                self.notice = Some(super::notice::Notice::plain(error.to_string()));
                return;
            }
        };
        let name = self
            .meta
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let matching: Vec<usize> = files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                name.as_ref()
                    .is_some_and(|name| file.names().contains(name))
            })
            .map(|(index, _)| index)
            .collect();
        let chosen = match (files.len(), matching.as_slice()) {
            (1, _) => Some(0),
            (_, [only]) => Some(*only),
            _ => None,
        };
        match chosen {
            Some(index) => self.apply_patch(&files[index]),
            None => self.patch_choice = Some(files),
        }
    }

    /// パッチを編集中の文書に当てる（R-04）。**1 回の編集にまとめる**（元に戻すで 1 回で戻る）
    pub(super) fn apply_patch(&mut self, file: &crate::diff::patch::FilePatch) {
        if self.read_only {
            self.refuse_edit();
            return;
        }
        let old = self.document.text().to_string();
        let result = crate::diff::patch::apply(&old, file);
        let total = result.applied + result.failed.len();
        if result.text != old {
            let (start, old_end, new_end) = changed_span(&old, &result.text);
            self.replace_range(
                start..old_end,
                old[start..old_end].to_owned(),
                result.text[start..new_end].to_owned(),
            );
            self.editor.anchor = None;
        }
        if result.failed.is_empty() {
            self.results = None;
            self.notice = Some(super::notice::Notice::plain(format!(
                "パッチを当てました（{total} か所）"
            )));
            return;
        }
        self.notice = Some(super::notice::Notice::plain(format!(
            "パッチの {} か所のうち {} か所は合わないので当てていません（下の一覧）",
            total,
            result.failed.len()
        )));
        self.results = Some(super::navigation::Results {
            title: "当てられなかった部分".to_owned(),
            items: result
                .failed
                .iter()
                .map(|failed| super::navigation::ResultItem {
                    line: failed.line,
                    column: 0,
                    label: format!("{} 番目 {}　{}", failed.number, failed.header, failed.first),
                })
                .collect(),
        });
    }

    /// どのファイルの部分を当てるか選ぶ帯（D-6）
    pub(super) fn patch_choice_view<'a>(
        &'a self,
        files: &'a [crate::diff::patch::FilePatch],
    ) -> Element<'a, Message> {
        let mut line = row![text("どのファイルの部分を当てますか").size(12)]
            .spacing(8)
            .align_y(iced::Alignment::Center);
        for (index, file) in files.iter().enumerate() {
            let [new, old] = file.names();
            let label = if new.is_empty() { old } else { new };
            line = line.push(
                button(text(format!("{label}（{} か所）", file.hunks.len())).size(12))
                    .padding([2, 8])
                    .on_press(Message::Diff(DiffMessage::ChoosePatch(Some(index)))),
            );
        }
        line.push(
            button(text("取り消し").size(12))
                .padding([2, 8])
                .style(button::secondary)
                .on_press(Message::Diff(DiffMessage::ChoosePatch(None))),
        )
        .into()
    }

    /// 1 列の面の通知（読むだけ。D-7）
    fn unified_action(&mut self, action: Action) -> Task<Message> {
        self.in_unified(|app| app.handle(Message::Editor(action)))
    }

    /// 1 列の面の文書を本文として扱いながら `f` を呼ぶ。**読み取り専用にして通す**
    /// 本文を `doc` と入れ替えて `f` を通す（v2.6.0 R-04 c。マージの面）。`read_only` なら書き換えを断る
    pub(super) fn in_doc<R>(
        &mut self,
        doc: &mut PaneDoc,
        read_only: bool,
        f: impl FnOnce(&mut App) -> R,
    ) -> R {
        let main_wrap = self.wrap.take();
        let main_folds = std::mem::take(&mut self.folds);
        let main_headings = std::mem::take(&mut self.fold_headings);
        std::mem::swap(&mut self.document, &mut doc.document);
        std::mem::swap(&mut self.editor, &mut doc.editor);
        std::mem::swap(&mut self.history, &mut doc.history);
        std::mem::swap(&mut self.meta, &mut doc.meta);
        let main_read_only = std::mem::replace(&mut self.read_only, read_only);
        self.diff_own = true;
        let notice = self.notice.take();
        let result = f(self);
        // 読むだけの面で書き換えようとしたら、その面の言葉で知らせる
        if read_only && self.notice.is_some() {
            self.notice = Some(super::notice::Notice::plain(
                "左右の面は読むだけです。結果の面で直してください".to_owned(),
            ));
        } else if self.notice.is_none() {
            self.notice = notice;
        }
        self.diff_own = false;
        self.read_only = main_read_only;
        std::mem::swap(&mut self.document, &mut doc.document);
        std::mem::swap(&mut self.editor, &mut doc.editor);
        std::mem::swap(&mut self.history, &mut doc.history);
        std::mem::swap(&mut self.meta, &mut doc.meta);
        self.wrap = main_wrap;
        self.folds = main_folds;
        self.fold_headings = main_headings;
        result
    }

    /// 1 列の比較の面で `f` を行う（検索。v2.8.0 R-02）
    pub(super) fn in_unified_pane<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        self.in_unified(f)
    }

    fn in_unified<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        let Some(mut screen) = self.diff_screen.take() else {
            return f(self);
        };
        let Some(doc) = screen.unified.as_mut() else {
            self.diff_screen = Some(screen);
            return f(self);
        };
        let main_wrap = self.wrap.take();
        let main_folds = std::mem::take(&mut self.folds);
        let main_headings = std::mem::take(&mut self.fold_headings);
        std::mem::swap(&mut self.document, &mut doc.document);
        std::mem::swap(&mut self.editor, &mut doc.editor);
        std::mem::swap(&mut self.history, &mut doc.history);
        std::mem::swap(&mut self.meta, &mut doc.meta);
        let read_only = std::mem::replace(&mut self.read_only, true);
        self.diff_own = true;
        let notice = self.notice.take();
        let result = f(self);
        // **読み取り専用の知らせを、1 列の面の言葉に変える**（「表示 → 読み取り専用」は外せない）
        if self.notice.is_some() {
            self.notice = Some(super::notice::Notice::plain(
                "1 列の表示は読むだけです。書き換えるときは、左右に並べる表示に切り替えてください"
                    .to_owned(),
            ));
        } else {
            self.notice = notice;
        }
        self.diff_own = false;
        self.read_only = read_only;
        std::mem::swap(&mut self.document, &mut doc.document);
        std::mem::swap(&mut self.editor, &mut doc.editor);
        std::mem::swap(&mut self.history, &mut doc.history);
        std::mem::swap(&mut self.meta, &mut doc.meta);
        self.wrap = main_wrap;
        self.folds = main_folds;
        self.fold_headings = main_headings;
        self.diff_screen = Some(screen);
        result
    }

    /// 面を触ったあと。**もう片方のスクロールを揃え**（R-02 f）、書き換えていれば比べ直す
    fn after_pane(&mut self, side: Side) -> Task<Message> {
        self.sync_panes(side);
        if self.diff_screen.as_ref().is_some_and(|screen| screen.stale) {
            return self.diff_recompute();
        }
        Task::none()
    }

    /// `from` の面の先頭の段に、もう片方を合わせる（横の位置も）。
    fn sync_panes(&mut self, from: Side) {
        let Some(mut screen) = self.diff_screen.take() else {
            return;
        };
        let to = match from {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        };
        let (top_line, top_segment, scroll_x) = {
            let editor = pane_editor(&screen, from, &self.editor);
            let segment = if editor.top_segment.0 == editor.top_line {
                editor.top_segment.1
            } else {
                0
            };
            (editor.top_line, segment, editor.scroll_x)
        };
        let row = screen.tables[index(from)].row_of(top_line, top_segment);
        let (line, segment) = screen.tables[index(to)].position(row);
        let editor = match screen.pane_mut(to).doc.as_mut() {
            Some(doc) => &mut doc.editor,
            None => &mut self.editor,
        };
        editor.top_line = line;
        editor.top_segment = (line, segment);
        editor.scroll_x = scroll_x;
        self.diff_screen = Some(screen);
    }

    /// 比べ直す。小さければその場で、大きければ裏の糸で
    pub(super) fn diff_recompute(&mut self) -> Task<Message> {
        let Some(screen) = self.diff_screen.as_ref() else {
            return Task::none();
        };
        if screen.computing {
            // 比べ終わったら、もう一度比べる（`Computed` で見る）
            if let Some(screen) = self.diff_screen.as_mut() {
                screen.stale = true;
            }
            return Task::none();
        }
        let text = |side: Side| match &screen.pane(side).doc {
            Some(doc) => doc.document.text().to_string(),
            None => self.document.text().to_string(),
        };
        let (left, right) = (text(Side::Left), text(Side::Right));
        let options = screen.options;
        let Some(screen) = self.diff_screen.as_mut() else {
            return Task::none();
        };
        screen.stale = false;
        screen.generation += 1;
        let generation = screen.generation;
        if left.len() + right.len() <= SYNC_LIMIT {
            let diff = Diff::compute(&left, &right, options);
            self.diff_apply(diff);
            return Task::none();
        }
        screen.computing = true;
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = sender.send(Diff::compute(&left, &right, options));
        });
        Task::future(async move { receiver.await.ok() }).map(move |diff| match diff {
            Some(diff) => Message::Diff(DiffMessage::Computed(generation, Box::new(diff))),
            None => Message::Noop,
        })
    }

    /// 比べた結果を面へ移す（空きの段の表と色）
    fn diff_apply(&mut self, diff: Diff) {
        let Some(mut screen) = self.diff_screen.take() else {
            return;
        };
        let (left_doc, right_doc) = (
            pane_document(&screen, Side::Left, &self.document),
            pane_document(&screen, Side::Right, &self.document),
        );
        let left_lines = left_doc.text().len_lines();
        let right_lines = right_doc.text().len_lines();

        // 行の中の違い（R-02 b）。**変わった行を前から組にする**
        let mut inline = InlineMap::new();
        'blocks: for block in diff.changes() {
            if let Block::Change { left, right } = block {
                for (l, r) in left.clone().zip(right.clone()) {
                    if inline.len() >= INLINE_PAIRS {
                        break 'blocks;
                    }
                    let pair =
                        crate::diff::inline(&line_text(left_doc, l), &line_text(right_doc, r));
                    inline.insert((l, r), pair);
                }
            }
        }

        let tables = [
            Wrap::spacers(left_lines, &diff.gaps(Side::Left)),
            Wrap::spacers(right_lines, &diff.gaps(Side::Right)),
        ];
        let marks = [
            marks_for(&diff, Side::Left, left_lines, &inline),
            marks_for(&diff, Side::Right, right_lines, &inline),
        ];
        // 差分の地図（R-02 e）。**左右に並べたときの段**で持つ
        let mut map = Vec::new();
        let mut row = 0usize;
        for block in diff.blocks() {
            let (left, right) = (block.left(), block.right());
            let rows = left.len().max(right.len());
            if block.is_change() {
                let both = !left.is_empty() && !right.is_empty();
                map.push(crate::render::diffmap::MapMark {
                    row,
                    rows,
                    left: (!left.is_empty()).then_some(if both { CHANGED } else { REMOVED }),
                    right: (!right.is_empty()).then_some(if both { CHANGED } else { ADDED }),
                });
            }
            row += rows;
        }
        // プレビューでの比較の色（v2.6.0 R-04 b）
        let preview_colors = if screen.is_preview() {
            [
                preview_marks(left_doc, &diff, Side::Left),
                preview_marks(right_doc, &diff, Side::Right),
            ]
        } else {
            [Vec::new(), Vec::new()]
        };
        // 1 列の面（読むだけ）。**面の文書を借りているうちに作る**
        let unified = screen
            .is_unified()
            .then(|| build_unified(&diff, left_doc, right_doc, &inline));
        screen.map = map;
        screen.preview_marks = preview_colors;
        if let Some((doc, marks, starts)) = unified {
            // 読んでいた場所を保つ（作り直しても先頭へ戻らない）
            let keep = screen
                .unified
                .as_ref()
                .map(|old| (old.editor.top_line, old.editor.cursor_line));
            let mut doc = doc;
            if let Some((top, cursor)) = keep {
                let last = doc.document.text().len_lines().saturating_sub(1);
                doc.editor.top_line = top.min(last);
                doc.editor.place_caret(cursor.min(last), 0);
            }
            screen.unified = Some(doc);
            screen.unified_marks = marks;
            screen.unified_starts = starts;
        } else {
            screen.unified = None;
        }
        screen.tables = tables;
        screen.marks = marks;
        screen.diff = diff;
        let count = screen.diff.change_count();
        screen.current = match screen.current {
            _ if count == 0 => None,
            Some(n) => Some(n.min(count - 1)),
            None => None,
        };
        screen.mark_current();
        self.diff_screen = Some(screen);
        // 表が変わったので、打鍵を受けている面に合わせて揃え直す
        if let Some(focus) = self.diff_screen.as_ref().map(|screen| screen.focus) {
            self.sync_panes(focus);
        }
    }

    /// 面を保存する（D-3・D-8）
    pub(super) fn save_pane(&mut self, side: Side) -> Task<Message> {
        let Some(screen) = self.diff_screen.as_mut() else {
            return Task::none();
        };
        if !screen.can_save(side) {
            self.notice = Some(super::notice::Notice::plain(
                "保存してある元（左）は保存できません。右へ写してから右を保存してください"
                    .to_owned(),
            ));
            return Task::none();
        }
        let Some(doc) = screen.pane_mut(side).doc.as_mut() else {
            // 編集中の文書（保存してある元と比べるときの右）。**ふだんの上書き保存と同じ道**
            return self.handle(Message::File(FileCommand::Save));
        };
        let Some(path) = doc.meta.path.clone() else {
            return Task::none();
        };
        let text = doc.document.text().to_string();
        match crate::io::save(&path, &text, &doc.meta.format) {
            Ok(()) => {
                doc.meta.dirty = false;
                self.notice = Some(super::notice::Notice::plain(format!(
                    "保存しました: {}",
                    path.display()
                )));
            }
            Err(error) => self.notice = Some(super::notice::Notice::plain(error.to_string())),
        }
        Task::none()
    }

    /// キャレットの点滅（`BlinkCaret` の刻みで呼ぶ）。**打鍵を受けている面だけ点滅させる**
    pub(super) fn diff_blink(&mut self) {
        let Some(screen) = self.diff_screen.as_mut() else {
            return;
        };
        let focus = screen.focus;
        let main_visible = self.editor.caret_visible;
        for side in [Side::Left, Side::Right] {
            let pane = screen.pane_mut(side);
            if let Some(doc) = pane.doc.as_mut() {
                doc.editor.caret_visible = side == focus && !doc.editor.caret_visible;
            } else if side != focus {
                self.editor.caret_visible = false;
            } else {
                // 編集中の文書は `BlinkCaret` がすでに切り替えている
                self.editor.caret_visible = main_visible;
            }
        }
    }

    /// 触った面のキャレットをすぐ出す（点滅の途中で消えたままにしない）
    fn diff_blink_reset(&mut self) {
        let Some(screen) = self.diff_screen.as_mut() else {
            return;
        };
        let focus = screen.focus;
        for side in [Side::Left, Side::Right] {
            match screen.pane_mut(side).doc.as_mut() {
                Some(doc) => doc.editor.caret_visible = side == focus,
                None => self.editor.caret_visible = side == focus,
            }
        }
    }

    /// diff の画面。
    pub(super) fn diff_view<'a>(&'a self, screen: &'a DiffScreen) -> Element<'a, Message> {
        let summary = if screen.computing {
            "比べています…".to_owned()
        } else if screen.diff.is_identical() {
            "違いはありません".to_owned()
        } else {
            format!("違い {} か所", screen.diff.change_count())
        };
        let mut notes = vec![summary];
        if screen.diff.approximate {
            notes.push("大きい文書のため、最短でない差分を出しています".to_owned());
        }
        let (left_meta, right_meta) = (
            pane_meta(screen, Side::Left, &self.meta),
            pane_meta(screen, Side::Right, &self.meta),
        );
        if left_meta.format.line_ending != right_meta.format.line_ending
            && !screen.options.ignore_eol
        {
            notes.push(format!(
                "改行コードが違います（左 {}・右 {}）",
                left_meta.format.line_ending.label(),
                right_meta.format.line_ending.label()
            ));
        }

        let small = |label: &'static str, message: Option<DiffMessage>| {
            button(text(label).size(12))
                .padding([4, 10])
                .on_press_maybe(message.map(Message::Diff))
        };
        let any = !screen.diff.is_identical();
        // 1 列・プレビューでは写さない（読むだけ）
        let chosen = screen.current.is_some() && !screen.is_unified() && !screen.is_preview();
        let position = match screen.current {
            Some(n) => format!("{} / {}", n + 1, screen.diff.change_count()),
            None => String::new(),
        };
        let header = row![
            text("比べる").size(15),
            text(notes.join("　")).size(12).width(Length::Fill),
            text(position).size(12),
            small("▲ 前の違い", any.then_some(DiffMessage::Previous)),
            small("▼ 次の違い", any.then_some(DiffMessage::Next)),
            small(
                "右へ写す →",
                chosen.then_some(DiffMessage::CopyTo(Side::Right))
            ),
            small(
                "← 左へ写す",
                chosen.then_some(DiffMessage::CopyTo(Side::Left))
            ),
            small("⇄ 左右を入れ替える", Some(DiffMessage::Swap)),
            small("パッチを作る…", any.then_some(DiffMessage::MakePatch)),
            button(text("閉じる").size(12))
                .padding([4, 12])
                .on_press(Message::Diff(DiffMessage::Close)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        // 無視する違い（R-02 d）。**設定と同じものを変える**（次に開いたときも同じ比べ方）
        use crate::io::settings::DiffLayout;
        let layout_button = |label: &'static str, layout: DiffLayout| {
            button(text(label).size(12))
                .padding([4, 10])
                .style(if self.settings.diff_layout == layout {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::Setting(super::SettingChange::DiffLayout(layout)))
        };
        let options = row![
            layout_button("左右に並べる", DiffLayout::SideBySide),
            layout_button("1 列に混ぜる", DiffLayout::Unified),
            layout_button("プレビュー", DiffLayout::Preview),
            fluent::pick_list(
                crate::io::settings::DiffWhitespace::ALL.to_vec(),
                Some(self.settings.diff_whitespace),
                |whitespace| Message::Setting(super::SettingChange::DiffWhitespace(whitespace)),
            )
            .text_size(12),
            fluent::checkbox(self.settings.diff_ignore_case)
                .label("大文字と小文字を無視")
                .text_size(12)
                .on_toggle(|on| Message::Setting(super::SettingChange::DiffIgnoreCase(on))),
            fluent::checkbox(self.settings.diff_ignore_eol)
                .label("改行コードを無視")
                .text_size(12)
                .on_toggle(|on| Message::Setting(super::SettingChange::DiffIgnoreEol(on))),
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center);
        let mut body = column![header, options].spacing(6).padding(6);
        if screen.confirm_close {
            body = body.push(
                row![
                    text("保存していない変更があります。閉じると失われます。").size(12),
                    button(text("保存せずに閉じる").size(12))
                        .padding([4, 10])
                        .on_press(Message::Diff(DiffMessage::ConfirmClose(true))),
                    button(text("戻る").size(12))
                        .padding([4, 10])
                        .on_press(Message::Diff(DiffMessage::ConfirmClose(false))),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
        }
        if let (true, Some(doc)) = (screen.is_unified(), screen.unified.as_ref()) {
            let (size, line_height) = self.editor_sizes();
            let view = EditorView::new(&doc.document, &doc.editor, |action| {
                Message::Diff(DiffMessage::Unified(action))
            })
            .font(self.editor_font())
            .sizes(size, line_height)
            .diff_marks(Some(&screen.unified_marks))
            // 行番号は行の頭に元のものを書いてあるので、通し番号は出さない
            .show_gutter(false)
            .accept_keys(self.open_menu.is_none() && !screen.confirm_close && !self.search_focused)
            .highlight_matches(self.search_highlight(super::SearchTarget::Unified))
            .match_colors(self.match_colors())
            .display(self.tab_width(), false, false, false);
            let titles = format!(
                "- {}　+ {}（読むだけ）",
                screen.left.title, screen.right.title
            );
            return body
                .push(container(text(titles).size(12)).padding([2, 4]))
                .push(container(view).width(Length::Fill).height(Length::Fill))
                .into();
        }
        // プレビューでの比較（v2.6.0 R-04 b。読むだけ）
        if screen.is_preview() {
            return body
                .push(
                    row![
                        self.diff_preview_pane(screen, Side::Left),
                        self.diff_preview_pane(screen, Side::Right),
                    ]
                    .spacing(6)
                    .height(Length::Fill),
                )
                .into();
        }
        // 地図に見えている範囲（左の面の先頭の段）
        let top = {
            let editor = pane_editor(screen, Side::Left, &self.editor);
            let segment = if editor.top_segment.0 == editor.top_line {
                editor.top_segment.1
            } else {
                0
            };
            screen.tables[0].row_of(editor.top_line, segment)
        };
        let map = crate::render::diffmap::DiffMap::new(
            screen.diff.total_rows(),
            &screen.map,
            (top, self.visible_rows()),
            |row| Message::Diff(DiffMessage::MapPick(row)),
        );
        body.push(
            row![
                self.pane_view(screen, Side::Left),
                map,
                self.pane_view(screen, Side::Right),
            ]
            .spacing(2)
            .height(Length::Fill),
        )
        .into()
    }

    /// プレビューでの比較の面（v2.6.0 R-04 b）
    fn diff_preview_pane<'a>(&'a self, screen: &'a DiffScreen, side: Side) -> Element<'a, Message> {
        let pane = screen.pane(side);
        let document = pane_document(screen, side, &self.document);
        let meta = pane_meta(screen, side, &self.meta);
        let i = index(side);
        let view = crate::render::PreviewView::new(document, &screen.previews[i], move |action| {
            Message::Diff(DiffMessage::Preview(side, action))
        })
        .zoom(super::preview_factor(&self.settings))
        .colors(self.preview_colors())
        .match_color(self.match_colors().0)
        // プレビューでの比較では、検索の語を左右とも塗る（v2.8.0 R-02。読むだけ）
        .highlight(if self.search.open {
            self.search.compiled.as_ref()
        } else {
            None
        })
        .embeds(&self.embeds)
        .base_dir(meta.base_dir())
        .block_marks(&screen.preview_marks[i]);
        column![
            container(text(format!("{}（読むだけ）", pane.title)).size(12)).padding([2, 4]),
            container(view).width(Length::Fill).height(Length::Fill),
        ]
        .width(Length::FillPortion(1))
        .into()
    }

    fn pane_view<'a>(&'a self, screen: &'a DiffScreen, side: Side) -> Element<'a, Message> {
        let pane = screen.pane(side);
        let (document, editor, meta) = match &pane.doc {
            Some(doc) => (&doc.document, &doc.editor, &doc.meta),
            None => (&self.document, &self.editor, &self.meta),
        };
        let mut title = pane.title.clone();
        if meta.dirty {
            title.push_str(" ●");
        }
        let save = button(text("保存").size(11))
            .padding([2, 10])
            .on_press_maybe(
                (screen.can_save(side) && meta.dirty)
                    .then_some(Message::Diff(DiffMessage::Save(side))),
            );
        let focused = screen.focus == side;
        let head = row![text(title).size(12).width(Length::Fill), save]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let (size, line_height) = self.editor_sizes();
        let i = index(side);
        let view = EditorView::new(document, editor, move |action| {
            Message::Diff(DiffMessage::Pane(side, action))
        })
        .font(self.editor_font())
        .sizes(size, line_height)
        .spacers(Some(&screen.tables[i]))
        .diff_marks(Some(&screen.marks[i]))
        .accept_keys(
            focused && self.open_menu.is_none() && !screen.confirm_close && !self.search_focused,
        )
        .highlight_matches(self.search_highlight(super::SearchTarget::Diff(side)))
        .match_colors(self.match_colors())
        .display(
            self.tab_width(),
            self.settings.show_invisibles,
            self.settings.show_gremlins,
            meta.format.line_ending == crate::io::LineEnding::Crlf,
        );
        column![
            container(head).padding([2, 4]),
            container(view).width(Length::Fill).height(Length::Fill),
        ]
        .width(Length::FillPortion(1))
        .into()
    }
}

fn pane_editor<'a>(screen: &'a DiffScreen, side: Side, main: &'a EditorState) -> &'a EditorState {
    match &screen.pane(side).doc {
        Some(doc) => &doc.editor,
        None => main,
    }
}

fn pane_document<'a>(screen: &'a DiffScreen, side: Side, main: &'a Document) -> &'a Document {
    match &screen.pane(side).doc {
        Some(doc) => &doc.document,
        None => main,
    }
}

fn pane_meta<'a>(screen: &'a DiffScreen, side: Side, main: &'a DocumentMeta) -> &'a DocumentMeta {
    match &screen.pane(side).doc {
        Some(doc) => &doc.meta,
        None => main,
    }
}

/// 読めない部分があったことの知らせ（開くときと同じ。§19.4）
fn lossy_notice(file: &LoadedFile) -> Option<super::notice::Notice> {
    file.lossy.then(|| {
        super::notice::Notice::plain(format!(
            "{} を {} として読めない部分がありました",
            file.path.display(),
            file.format.encoding.label()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_names_get_their_folder() {
        let (left, right) = titles(
            std::path::Path::new("/a/one/memo.md"),
            std::path::Path::new("/a/two/memo.md"),
        );
        assert_eq!(
            (left.as_str(), right.as_str()),
            ("one/memo.md", "two/memo.md")
        );
        let (left, right) = titles(
            std::path::Path::new("/a/old.md"),
            std::path::Path::new("/a/new.md"),
        );
        assert_eq!((left.as_str(), right.as_str()), ("old.md", "new.md"));
    }

    #[test]
    fn marks_color_each_side() {
        let diff = Diff::compute("a\nb\nc\n", "a\nB\nc\nd\n", Options::default());
        let inline = InlineMap::new();
        let left = marks_for(&diff, Side::Left, 4, &inline);
        let right = marks_for(&diff, Side::Right, 5, &inline);
        assert_eq!(left.lines[1], LineMark::Changed);
        assert_eq!(right.lines[1], LineMark::Changed);
        assert_eq!(right.lines[3], LineMark::Only, "右にだけある行");
        assert_eq!(left.lines[0], LineMark::Same);
        assert_eq!(left.only, REMOVED);
        assert_eq!(right.only, ADDED);
    }

    fn loaded(path: &str, text: &str) -> Box<LoadedFile> {
        Box::new(LoadedFile {
            path: PathBuf::from(path),
            text: text.to_owned(),
            format: crate::io::FileFormat::for_new_document(),
            oversized: false,
            lossy: false,
            truncated: None,
        })
    }

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn open_files(app: &mut App, left: &str, right: &str) {
        let _ = app.update(Message::Diff(DiffMessage::LoadedFiles(Ok((
            loaded("/x/left.md", left),
            loaded("/x/right.md", right),
        )))));
    }

    fn pane_text(app: &App, side: Side) -> String {
        let screen = app.diff_screen.as_ref().expect("開いている");
        match &screen.pane(side).doc {
            Some(doc) => doc.document.text().to_string(),
            None => app.document.text().to_string(),
        }
    }

    /// 裏の走査をその場で行い、結果を渡す（試験用）
    fn finish_search(app: &mut App, side: Side) {
        let pattern = app.search.pattern().expect("語");
        let rope = match &app.diff_screen.as_ref().unwrap().pane(side).doc {
            Some(doc) => doc.document.text().clone(),
            None => app.document.text().clone(),
        };
        let generation = app.search.generation;
        let _ = app.update(Message::SearchFound {
            generation,
            found: Ok(pattern.find_all(&rope)),
            jump: true,
        });
    }

    /// 比べる画面では、いま入力のある面を探す。面を移ると探し直し、閉じると検索の欄も閉じる（v2.8.0 R-02）
    #[test]
    fn search_follows_the_focused_pane() {
        let mut app = app_with("本文 x\n");
        let _ = app.update(Message::OpenSearch);
        assert!(app.search.open);
        open_files(&mut app, "a\nxa\nx\n", "a\nB\n");
        assert!(!app.search.open, "開いたら編集中の文書の検索は閉じる");
        // 左の面を押してから探す
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Left,
            Action::Move {
                movement: crate::render::CursorMove::DocumentStart,
                select: false,
            },
        )));
        let _ = app.update(Message::OpenSearch);
        assert!(app.search.open, "比べる画面でも開く");
        let _ = app.update(Message::SearchInput("x".to_owned()));
        finish_search(&mut app, Side::Left);
        assert_eq!(app.search.matches.len(), 2);
        assert_eq!(
            app.search_highlight(super::super::SearchTarget::Diff(Side::Left))
                .0
                .len(),
            2
        );
        assert!(app
            .search_highlight(super::super::SearchTarget::Diff(Side::Right))
            .0
            .is_empty());
        // 次の一致へ移ると、左の面のキャレットが動く
        let _ = app.update(Message::SearchStep(true));
        let caret = app
            .diff_screen
            .as_ref()
            .unwrap()
            .pane(Side::Left)
            .doc
            .as_ref()
            .unwrap()
            .editor
            .cursor_line;
        assert_eq!(caret, 2, "2 件目（3 行目の x）");
        // 右の面へ移ると、一致を捨てて右で探し直す
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Right,
            Action::Move {
                movement: crate::render::CursorMove::DocumentStart,
                select: false,
            },
        )));
        assert!(app.search.matches.is_empty());
        finish_search(&mut app, Side::Right);
        assert!(app.search.matches.is_empty(), "右には x が無い");
        // 置き換えは面の文書を書き換える。編集中の文書は変わらない
        let _ = app.update(Message::SearchInput("B".to_owned()));
        finish_search(&mut app, Side::Right);
        let _ = app.update(Message::ReplaceInput("C".to_owned()));
        let _ = app.update(Message::ReplaceAll);
        assert_eq!(pane_text(&app, Side::Right), "a\nC\n");
        assert_eq!(app.document.text().to_string(), "本文 x\n");
        // Esc はまず検索の欄を閉じる
        assert!(matches!(app.escape_message(), Some(Message::CloseSearch)));
        let _ = app.update(Message::CloseSearch);
        assert!(matches!(
            app.escape_message(),
            Some(Message::Diff(DiffMessage::Close))
        ));
        let _ = app.update(Message::OpenSearch);
        let _ = app.update(Message::Diff(DiffMessage::Close));
        if app.diff_screen.is_none() {
            assert!(!app.search.open, "閉じたら検索の欄も閉じる");
        }
    }

    /// **面を書き換えても、編集中の文書は変わらない**（ファイルとファイル）
    #[test]
    fn typing_in_a_pane_edits_only_that_pane() {
        let mut app = app_with("本文\n");
        open_files(&mut app, "a\nb\n", "a\nB\n");
        assert_eq!(app.diff_screen.as_ref().unwrap().diff.change_count(), 1);

        // 左の 1 行目の頭を押して打つ
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Left,
            Action::Move {
                movement: crate::render::CursorMove::DocumentEnd,
                select: false,
            },
        )));
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Left,
            Action::Insert("c".to_owned()),
        )));
        assert_eq!(pane_text(&app, Side::Left), "a\nb\nc");
        assert_eq!(pane_text(&app, Side::Right), "a\nB\n");
        assert_eq!(
            app.document.text().to_string(),
            "本文\n",
            "編集中の文書は変わらない"
        );
        assert!(!app.meta.dirty);
        let screen = app.diff_screen.as_ref().unwrap();
        assert!(screen.left.doc.as_ref().unwrap().meta.dirty);
        assert_eq!(screen.focus, Side::Left, "触った面に打鍵が移る");
        // 比べ直してある（b→B と、足した c）
        assert_eq!(screen.diff.change_count(), 1);
        assert!(!screen.stale);
    }

    /// 元に戻すは、打鍵を受けている面に効く
    #[test]
    fn undo_goes_to_the_focused_pane() {
        let mut app = app_with("本文\n");
        open_files(&mut app, "a\n", "a\n");
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Right,
            Action::Insert("x".to_owned()),
        )));
        assert_eq!(pane_text(&app, Side::Right), "xa\n");
        let _ = app.update(Message::Undo);
        assert_eq!(pane_text(&app, Side::Right), "a\n");
        assert_eq!(app.document.text().to_string(), "本文\n");
    }

    /// **保存してある元と比べるとき、右は編集中の文書そのもの**（D-9）
    #[test]
    fn the_right_pane_of_saved_is_the_document() {
        let mut app = app_with("a\nb\n");
        let _ = app.update(Message::Diff(DiffMessage::LoadedSaved(Ok(loaded(
            "/x/memo.md",
            "a\nb\n",
        )))));
        assert!(app.diff_screen.as_ref().unwrap().diff.is_identical());
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Right,
            Action::Insert("z".to_owned()),
        )));
        assert_eq!(app.document.text().to_string(), "za\nb\n");
        assert!(app.meta.dirty);
        assert_eq!(app.diff_screen.as_ref().unwrap().diff.change_count(), 1);
        // 左は保存できない（D-8）
        assert!(!app.diff_screen.as_ref().unwrap().can_save(Side::Left));
        // 閉じても、直したところは残る
        let _ = app.update(Message::Diff(DiffMessage::Close));
        assert!(
            app.diff_screen.is_none(),
            "左を直していなければ確かめずに閉じる"
        );
        assert_eq!(app.document.text().to_string(), "za\nb\n");
    }

    /// 保存していない面があれば、閉じる前に確かめる
    #[test]
    fn closing_with_unsaved_panes_asks_first() {
        let mut app = app_with("");
        open_files(&mut app, "a\n", "a\n");
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Left,
            Action::Insert("x".to_owned()),
        )));
        let _ = app.update(Message::Diff(DiffMessage::Close));
        assert!(app.diff_screen.as_ref().unwrap().confirm_close);
        let _ = app.update(Message::Diff(DiffMessage::ConfirmClose(false)));
        assert!(app.diff_screen.is_some());
        let _ = app.update(Message::Diff(DiffMessage::Close));
        let _ = app.update(Message::Diff(DiffMessage::ConfirmClose(true)));
        assert!(app.diff_screen.is_none());
    }

    /// 編集中の文書を差し替える操作は受け付けない
    #[test]
    fn opening_another_file_is_blocked() {
        let mut app = app_with("本文\n");
        open_files(&mut app, "a\n", "b\n");
        let _ = app.update(Message::File(FileCommand::New));
        assert_eq!(app.document.text().to_string(), "本文\n");
        assert!(app.diff_screen.is_some());
    }

    /// 片方を巻き上げると、もう片方も同じ段へついてくる（R-02 f）
    #[test]
    fn scrolling_one_pane_moves_the_other() {
        let left: String = (0..50).map(|i| format!("{i}\n")).collect();
        let right: String = (0..10)
            .map(|i| format!("足した {i}\n"))
            .chain((0..50).map(|i| format!("{i}\n")))
            .collect();
        let mut app = app_with("");
        open_files(&mut app, &left, &right);
        let _ = app.update(Message::Diff(DiffMessage::Pane(
            Side::Right,
            Action::ScrollTo { top_line: 20 },
        )));
        let screen = app.diff_screen.as_ref().unwrap();
        let left_editor = &screen.left.doc.as_ref().unwrap().editor;
        // 右の 20 行目は左の 10 行目（左には 10 段の空きがある）
        assert_eq!(left_editor.top_line, 10);
    }

    fn current(app: &App) -> Option<usize> {
        app.diff_screen.as_ref().unwrap().current
    }

    /// 次の違い・前の違いへ移る。端では止まる（R-02 a）
    #[test]
    fn stepping_through_changes() {
        let mut app = app_with("");
        open_files(&mut app, "a\nb\nc\nd\ne\n", "a\nB\nc\nD\ne\n");
        assert_eq!(app.diff_screen.as_ref().unwrap().diff.change_count(), 2);
        assert_eq!(current(&app), None);
        let _ = app.update(Message::Diff(DiffMessage::Next));
        assert_eq!(current(&app), Some(0));
        let screen = app.diff_screen.as_ref().unwrap();
        assert_eq!(screen.left.doc.as_ref().unwrap().editor.cursor_line, 1);
        assert_eq!(screen.marks[0].current, Some(1..2), "選んだ違いに枠");
        let _ = app.update(Message::Diff(DiffMessage::Next));
        let _ = app.update(Message::Diff(DiffMessage::Next));
        assert_eq!(current(&app), Some(1), "最後で止まる");
        let _ = app.update(Message::Diff(DiffMessage::Previous));
        assert_eq!(current(&app), Some(0));
        // F7 の割り当てからも同じところへ届く
        let _ = app.update(crate::app::keymap::Command::DiffNext.message());
        assert_eq!(current(&app), Some(1));
    }

    /// **写すと、その違いは消え、次の違いを選ぶ**。元に戻すで 1 回で戻る（R-02 c）
    #[test]
    fn copying_a_change_to_the_other_side() {
        let mut app = app_with("");
        open_files(&mut app, "a\nb\nc\nd\n", "a\nB\nc\nD\nx\n");
        let _ = app.update(Message::Diff(DiffMessage::Next));
        let _ = app.update(Message::Diff(DiffMessage::CopyTo(Side::Right)));
        assert_eq!(pane_text(&app, Side::Right), "a\nb\nc\nD\nx\n");
        let screen = app.diff_screen.as_ref().unwrap();
        assert_eq!(screen.diff.change_count(), 1);
        assert_eq!(screen.current, Some(0), "残った違いを選ぶ");
        assert!(screen.right.doc.as_ref().unwrap().meta.dirty);
        // 残った違い（d と D・x）を左へ写すと、右と同じになる
        let _ = app.update(Message::Diff(DiffMessage::CopyTo(Side::Left)));
        assert_eq!(pane_text(&app, Side::Left), "a\nb\nc\nD\nx\n");
        assert!(app.diff_screen.as_ref().unwrap().diff.is_identical());
        // 左の写しを戻す
        let _ = app.update(Message::Undo);
        assert_eq!(pane_text(&app, Side::Left), "a\nb\nc\nd\n");
    }

    /// 片側にだけある行も写せる（無いほうへ足す・あるほうから消す）
    #[test]
    fn copying_added_lines() {
        let mut app = app_with("");
        open_files(&mut app, "a\nc\n", "a\nb\nc\n");
        let _ = app.update(Message::Diff(DiffMessage::Next));
        let _ = app.update(Message::Diff(DiffMessage::CopyTo(Side::Left)));
        assert_eq!(pane_text(&app, Side::Left), "a\nb\nc\n");
        let _ = app.update(Message::Undo);
        let _ = app.update(Message::Diff(DiffMessage::Next));
        let _ = app.update(Message::Diff(DiffMessage::CopyTo(Side::Right)));
        assert_eq!(pane_text(&app, Side::Right), "a\nc\n");
    }

    /// 左右を入れ替えると、保存できない面も一緒に移る（D-4・D-8）
    #[test]
    fn swapping_keeps_each_pane_with_its_rules() {
        let mut app = app_with("a\nB\n");
        let _ = app.update(Message::Diff(DiffMessage::LoadedSaved(Ok(loaded(
            "/x/memo.md",
            "a\nb\n",
        )))));
        let _ = app.update(Message::Diff(DiffMessage::Swap));
        let screen = app.diff_screen.as_ref().unwrap();
        assert!(screen.left.doc.is_none(), "編集中の文書が左へ");
        assert!(
            !screen.can_save(Side::Right),
            "保存してある元は右でも保存できない"
        );
        assert!(screen.can_save(Side::Left));
        assert_eq!(screen.diff.change_count(), 1);
    }

    /// 無視する違いを変えると、その場で比べ直し、設定にも残る（R-02 d）
    #[test]
    fn ignoring_differences_recompares_at_once() {
        let mut app = app_with("");
        open_files(&mut app, "a  b\nX\n", "a b\nx\n");
        assert_eq!(app.diff_screen.as_ref().unwrap().diff.change_count(), 1);
        let _ = app.update(Message::Setting(
            super::super::SettingChange::DiffWhitespace(
                crate::io::settings::DiffWhitespace::IgnoreChanges,
            ),
        ));
        let _ = app.update(Message::Setting(
            super::super::SettingChange::DiffIgnoreCase(true),
        ));
        assert!(app.diff_screen.as_ref().unwrap().diff.is_identical());
        assert!(app.settings.diff_ignore_case);
        // 次に開いたときも同じ比べ方
        let _ = app.update(Message::Diff(DiffMessage::Close));
        open_files(&mut app, "A\n", "a\n");
        assert!(app.diff_screen.as_ref().unwrap().diff.is_identical());
    }

    fn unified_text(app: &App) -> String {
        let screen = app.diff_screen.as_ref().unwrap();
        screen
            .unified
            .as_ref()
            .expect("1 列の面がある")
            .document
            .text()
            .to_string()
    }

    /// **1 列に混ぜて出す**。消えた行を先、足された行を後に、元の行番号と印を付ける（D-7）
    #[test]
    fn the_unified_layout_mixes_both_sides() {
        let mut app = app_with("");
        open_files(&mut app, "a\nb\nc", "a\nB\nc");
        let _ = app.update(Message::Setting(super::super::SettingChange::DiffLayout(
            crate::io::settings::DiffLayout::Unified,
        )));
        let text = unified_text(&app);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            vec![
                "    1     1   a",
                "    2       - b",
                "          2 + B",
                "    3     3   c",
            ]
        );
        let screen = app.diff_screen.as_ref().unwrap();
        assert_eq!(screen.unified_marks.lines[1], LineMark::Only);
        assert_eq!(screen.unified_marks.lines[2], LineMark::Other);
        assert_eq!(screen.unified_starts, vec![1]);
        // 次の違いへ移ると、1 列の面のキャレットが違いの行へ
        let _ = app.update(Message::Diff(DiffMessage::Next));
        let screen = app.diff_screen.as_ref().unwrap();
        assert_eq!(screen.unified.as_ref().unwrap().editor.cursor_line, 1);
    }

    /// 1 列の面は読むだけ。打っても変わらず、左右の文書も変わらない
    #[test]
    fn the_unified_layout_is_read_only() {
        let mut app = app_with("");
        open_files(&mut app, "a\n", "b\n");
        let _ = app.update(Message::Setting(super::super::SettingChange::DiffLayout(
            crate::io::settings::DiffLayout::Unified,
        )));
        let before = unified_text(&app);
        let _ = app.update(Message::Diff(DiffMessage::Unified(Action::Insert(
            "x".to_owned(),
        ))));
        let _ = app.update(Message::Editor(Action::Insert("y".to_owned())));
        assert_eq!(unified_text(&app), before);
        assert_eq!(pane_text(&app, Side::Left), "a\n");
        assert_eq!(pane_text(&app, Side::Right), "b\n");
        assert!(app
            .notice
            .as_ref()
            .is_some_and(|n| n.text.contains("読むだけ")));
        // 左右に戻すと 1 列の面は捨てる
        let _ = app.update(Message::Setting(super::super::SettingChange::DiffLayout(
            crate::io::settings::DiffLayout::SideBySide,
        )));
        assert!(app.diff_screen.as_ref().unwrap().unified.is_none());
    }

    /// **外（ほかの窓・試験の口）から比べ方が変わっても、出している画面に移す**
    #[test]
    fn settings_from_outside_reach_the_screen() {
        let mut app = app_with("");
        open_files(&mut app, "a\n", "b\n");
        let _ = app.update(Message::Automation(
            r#"{"id":1,"cmd":"set_setting","key":"diff_layout","value":"unified"}"#.to_owned(),
        ));
        assert!(app.diff_screen.as_ref().unwrap().is_unified());
        assert!(app.diff_screen.as_ref().unwrap().unified.is_some());
        let _ = app.update(Message::Automation(
            r#"{"id":2,"cmd":"set_setting","key":"diff_ignore_case","value":true}"#.to_owned(),
        ));
        assert!(app.diff_screen.as_ref().unwrap().options.ignore_case);
    }

    fn temp_patch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdview-patch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("作れる");
        dir.join(name)
    }

    /// **作ったパッチを左の文書に当てると、右の文書になる**。元に戻すで 1 回で戻る（R-03 / R-04）
    #[test]
    fn a_made_patch_turns_left_into_right() {
        for format in [
            crate::io::settings::PatchFormat::Unified,
            crate::io::settings::PatchFormat::Context,
        ] {
            let left = "# 手順\n\n1. 開く\n2. 閉じる\n";
            let right = "# 手順\n\n1. 開く\n2. 保存する\n3. 閉じる\n";
            let mut app = app_with(left);
            app.settings.patch_format = format;
            open_files(&mut app, left, right);
            let path = temp_patch(&format!("{format:?}.patch"));
            app.write_patch(&path);
            let _ = app.update(Message::Diff(DiffMessage::Close));
            let text = std::fs::read_to_string(&path).expect("書けている");
            assert!(
                text.contains("+3. 閉じる") || text.contains("! 3. 閉じる") || text.contains("+ "),
                "{text}"
            );

            app.choose_patch(&text);
            assert_eq!(app.document.text().to_string(), right, "{format:?}");
            assert!(app.meta.dirty);
            let _ = app.update(Message::Undo);
            assert_eq!(app.document.text().to_string(), left, "1 回で戻る");
        }
        // 書き出したパッチを片付ける
        if let Some(dir) = temp_patch("x").parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// 合わない部分は当てずに一覧へ（R-04）
    #[test]
    fn mismatched_hunks_are_listed() {
        let patch = crate::diff::patch::write(
            "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n",
            "a\nB\nc\nd\ne\nf\ng\nh\ni\nJ\n",
            "x.md",
            "x.md",
            crate::diff::patch::Format::Unified,
        );
        let mut app = app_with("a\nb\nc\nd\ne\nf\ng\nh\nI\nj\n");
        app.choose_patch(&patch);
        assert!(
            app.document.text().to_string().starts_with("a\nB\n"),
            "1 つ目は当たる"
        );
        let results = app.results.as_ref().expect("一覧が出る");
        assert_eq!(results.items.len(), 1);
        assert!(results.items[0].label.starts_with("2 番目"));
    }

    /// 複数のファイルが入っていて、名前で決められなければ選んでもらう（D-6）
    #[test]
    fn several_files_ask_which_one() {
        let text =
            "--- a.md\n+++ a.md\n@@ -1 +1 @@\n-a\n+A\n--- b.md\n+++ b.md\n@@ -1 +1 @@\n-a\n+B\n";
        let mut app = app_with("a\n");
        app.choose_patch(text);
        assert_eq!(app.patch_choice.as_ref().map(Vec::len), Some(2));
        assert_eq!(app.document.text().to_string(), "a\n", "選ぶまで当てない");
        let _ = app.update(Message::Diff(DiffMessage::ChoosePatch(Some(1))));
        assert_eq!(app.document.text().to_string(), "B\n");
        assert!(app.patch_choice.is_none());
        // 名前が合えば聞かない
        let mut app = app_with("a\n");
        app.meta = DocumentMeta::opened(
            PathBuf::from("/x/b.md"),
            crate::io::FileFormat::for_new_document(),
        );
        app.choose_patch(text);
        assert_eq!(app.document.text().to_string(), "B\n");
    }

    #[test]
    fn the_changed_span_respects_characters() {
        assert_eq!(changed_span("日本語", "日本人"), (6, 9, 9));
        assert_eq!(changed_span("abc", "abc"), (3, 3, 3));
        assert_eq!(changed_span("abc", "abXc"), (2, 2, 3));
    }

    /// 地図の印は、左右に並べたときの段で持つ（R-02 e）
    #[test]
    fn the_map_marks_each_change() {
        let mut app = app_with("");
        open_files(&mut app, "a\nb\nc\n", "a\nc\nd\n");
        let screen = app.diff_screen.as_ref().unwrap();
        assert_eq!(screen.map.len(), 2);
        assert_eq!((screen.map[0].row, screen.map[0].rows), (1, 1));
        assert_eq!(screen.map[0].left, Some(REMOVED));
        assert_eq!(screen.map[0].right, None, "右には行が無い");
        assert_eq!(screen.map[1].right, Some(ADDED));
    }

    #[test]
    fn editing_messages_go_to_the_pane() {
        assert_eq!(route(&Message::Undo), Route::Pane);
        assert_eq!(route(&Message::Editor(Action::Backspace)), Route::Pane);
        assert_eq!(route(&Message::File(FileCommand::Save)), Route::Save);
        assert_eq!(route(&Message::File(FileCommand::Open)), Route::Blocked);
        assert_eq!(
            route(&Message::OpenSearch),
            Route::Pane,
            "検索は面で（v2.8.0 R-02）"
        );
        assert_eq!(route(&Message::CloseSearch), Route::Pass);
        assert_eq!(route(&Message::OpenGoto), Route::Blocked);
        assert_eq!(route(&Message::BlinkCaret), Route::Pass);
    }
}
