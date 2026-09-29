//! アプリ層。状態・メッセージ・更新（§13.1）。
//!
//! P1（骨格）で扱うのは次の 4 つ。
//!
//!   - ファイルを開く（起動引数）
//!   - エディタの表示とスクロール
//!   - 文字入力と後退（ロープへの適用・索引の差分更新）
//!   - **IME**（OPEN-207 の確認）

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use std::path::Path;

use crate::document::Document;
use crate::embed::{DiagramRenderer, Dispatch, EmbedPool, ImageRenderer, MathRenderer};
use crate::io::settings::{Settings, ThemePreference};
use crate::layout::{Metrics, ScrollAnchor};
pub use export_dialog::{Format, RangeKind};
pub use file::Answer;
use file::{decide, needs_save_as, DocumentMeta, Next, Pending};
mod cursor;
// 検索バーの状態（§8.2）
mod search;
// エディタとプレビューの対応づけ（§16.14）
mod sync;
// 出力範囲のダイアログ（SCR-005）
mod export_dialog;
// 画面に出す知らせ
mod notice;
// ファイル選択ダイアログの組み立て（§14.1）
mod picker;
// 目次の中身（受入条件 §23.1）
mod toc;
// 文書の状態とファイル操作の判断（§18.2）
mod file;

use crate::render::{
    Action, Divider, EditorState, EditorView, FileCommand, ImeAction, Message, PreviewAction,
    PreviewState, PreviewView, ViewMode,
};

/// 目次の幅の下限・上限（px）。**設定ファイルが受け付ける範囲と揃える**（§13.5）。
/// 揃えないと、掴んで広げた幅が次回起動時に捨てられる
const TOC_MIN_WIDTH: f32 = 120.0;
const TOC_MAX_WIDTH: f32 = 800.0;

/// 分割比の下限・上限。同じく設定ファイルの範囲に揃える
const SPLIT_MIN: f32 = 0.1;
const SPLIT_MAX: f32 = 0.9;

/// 検索の入力欄の識別子。`Ctrl + F` で焦点を移すために要る
fn search_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("search-input")
}

/// 知らせに出すパスの長さ（文字）。**これを超えたら真ん中を省く**
const NOTICE_PATH_CHARS: usize = 60;

/// 分割比を動かすときの、想定しうる最小の幅（px）。
///
/// **0 除算を避ける。** 窓が極端に狭いときでも比率は動かせる必要がある
const MIN_SPLIT_AREA: f32 = 200.0;

pub struct App {
    document: Document,
    editor: EditorState,
    preview: PreviewState,
    mode: ViewMode,
    /// 文書の素性（パス・書式・未保存フラグ）
    meta: DocumentMeta,
    /// 未保存の確認を出している最中の操作（§18.2）
    pending: Option<Pending>,
    /// 画面に出す知らせ（読み込み・保存・出力の結果）
    notice: Option<notice::Notice>,
    /// 確認ダイアログの答え（保留中の操作へ渡す）
    queued_answer: Option<Answer>,
    /// 未保存の確認を出しているか（§18.2 / SCR-004）
    confirming: bool,
    /// 保存される設定（§13.5）
    settings: Settings,
    /// OS の外観が暗いか。**起動時に 1 度だけ見る**
    ///
    /// iced は OS のテーマを内部で追うが、アプリへ渡す口が無い。
    /// §10.4 は「検知できない環境では次回起動時に反映する」
    /// としており、その扱いに寄せる
    system_dark: bool,
    /// 設定を変えた時刻。**1 秒のデバウンスで書く**（§13.5）
    settings_touched: Option<std::time::Instant>,
    metrics: Metrics,
    /// 直近の編集にかかった時間（ms）。P1 の確認用に画面へ出す
    last_edit_ms: f64,
    /// 直近の編集が末尾まで走査し直したか（§12.8 の最悪ケース）
    last_edit_full_rescan: bool,
    /// IME イベントの記録を画面に出すか（P1 の確認用）
    show_ime_log: bool,
    /// スクロール性能の計測（`--bench-scroll`）。PERF-02
    bench: Option<Bench>,
    /// 図・数式・画像の非同期描画（§4.3）
    embeds: EmbedPool,
    /// 目次の中身。**編集のたびに作り直さない**（閉じている間は空）
    toc: Vec<toc::Entry>,
    /// いまのウィンドウ幅。つまみの px を比率へ直すために持つ
    window_width: f32,
    /// 検索の状態（§8.2）
    search: search::SearchState,
    /// 出力の進み具合（§17.10）。出力中だけ入る
    export: Option<ExportJob>,
    /// 出力範囲を選んでいる最中（SCR-005）
    export_dialog: Option<export_dialog::ExportDialog>,
    /// 保存先を聞いたあと、保留していた操作へ戻るか
    resume_after_save: bool,
    /// ダイアログを出している最中か。
    ///
    /// **UI は止まらない**（§10.31）ので、押しっぱなしで 2 つ開けてしまう。
    /// 開いている間はファイル操作の口を閉じる
    picking: bool,
}

/// 動いている出力 1 件。
///
/// **原子変数で受け渡す。** 進み具合は 1 秒に何度も変わるので、
/// メッセージにすると画面の更新がそれだけで埋まる
struct ExportJob {
    done: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    total: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: std::time::Instant,
}

impl ExportJob {
    fn new() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        use std::sync::Arc;
        Self {
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            started: std::time::Instant::now(),
        }
    }

    /// 画面に出す文字（§17.10 の「x / y ページ」と経過時間）。
    fn label(&self) -> String {
        use std::sync::atomic::Ordering;
        let done = self.done.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        let elapsed = self.started.elapsed().as_secs_f64();
        if total == 0 {
            // 割り付けの前。**ページ数はまだ分からない**
            format!("PDF を出力しています… 準備中（{elapsed:.1} 秒）")
        } else {
            format!("PDF を出力しています… {done} / {total} ページ（{elapsed:.1} 秒）")
        }
    }

    fn watch(&self) -> ExportWatcher {
        ExportWatcher {
            done: self.done.clone(),
            total: self.total.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

/// ワーカーから進み具合を書き込む口。
struct ExportWatcher {
    done: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    total: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crate::export::ExportWatch for ExportWatcher {
    fn total(&self, pages: usize) {
        self.total
            .store(pages, std::sync::atomic::Ordering::Relaxed);
    }

    fn done(&self, pages: usize) {
        self.done.store(pages, std::sync::atomic::Ordering::Relaxed);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// スクロール計測の状態。
///
/// **PoC（§6.2）と同じ条件**にして直接比較できるようにする。
/// 1 フレーム 7 行ずつ送り、15 秒間続ける。
struct Bench {
    started: std::time::Instant,
    ticks: u64,
    frames: std::cell::Cell<u64>,
    /// 1 打鍵ごとに測る（`--bench-edit`）。OPEN-201
    ///
    /// **スクロールではなく編集を駆動する。** 打鍵からプレビューの
    /// レイアウトが終わるまでを直接測るのが目的である。
    edit_mode: bool,
    /// 打鍵した時刻。プレビューの測定結果が返ってきた時点で止める
    pending: Option<std::time::Instant>,
    /// 打鍵ごとのレイテンシ（ms）
    latencies: Vec<f64>,
}

/// 保存先のダイアログ（名前を付けて保存）。
fn save_picker(meta: &DocumentMeta) -> picker::Picker {
    let name = meta
        .path
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "無題.md".to_owned());

    picker::Picker::save("Markdown", "md", name)
        .in_directory(meta.base_dir().map(Path::to_path_buf))
}

/// 出力先のダイアログ。**既定の名前は「題名 + 拡張子」**（§17A.4）。
fn export_picker(meta: &DocumentMeta, format: Format) -> picker::Picker {
    let name = format!("{}.{}", meta.display_title(), format.extension());
    picker::Picker::save(format.label(), format.extension(), name)
        .in_directory(meta.base_dir().map(Path::to_path_buf))
}

/// 新規文書の中身。
const NEW_DOCUMENT: &str = "# 新しい文書\n\n";

/// 計測する秒数。PoC と揃える。
const BENCH_SECS: f64 = 15.0;

/// 計測の駆動間隔（ms）。既定は PoC と同じ 16ms。
///
/// `BENCH_TICK_MS` で変えられる。**vsync（16.67ms）との食い違いが
/// フレーム数に効いていないか**を切り分けるために外から指定できるようにしてある。
fn bench_tick_ms() -> u64 {
    std::env::var("BENCH_TICK_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

/// 計測中に行番号を描くか。`BENCH_NO_GUTTER=1` で外す。
///
/// PoC は本文のみを描いていた。**描画量の差がフレーム数に効いているか**の切り分け用。
fn bench_show_gutter() -> bool {
    std::env::var("BENCH_NO_GUTTER").as_deref() != Ok("1")
}
/// 1 フレームに送る行数。PoC と揃える。
const BENCH_LINES_PER_FRAME: usize = 7;

/// レイテンシの中央値・最悪値・件数。
///
/// **平均は使わない。** 起動直後の 1 回や、たまたま重なった GC 相当の処理に
/// 引きずられる。中央値と最悪値の両方を出す。
fn latency_summary(values: &mut [f64]) -> (f64, f64, usize) {
    if values.is_empty() {
        return (0.0, 0.0, 0);
    }
    values.sort_by(|a, b| a.partial_cmp(b).expect("NaN は入らない"));
    let median = values[values.len() / 2];
    let worst = *values.last().expect("空でない");
    (median, worst, values.len())
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        // **読み込みに失敗しても既定値で起動する**（§13.5）
        let settings = Settings::load();
        let path = std::env::args().nth(1).filter(|arg| !arg.starts_with("--"));

        // **起動時の読み込みは同期で行う。** 窓が出る前なので UI は止まらない。
        // 開けなければ新規文書として起動する（起動しないよりよい）
        let loaded = path
            .as_deref()
            .map(std::path::Path::new)
            .map(crate::io::load);

        let (text, meta, notice) = match loaded {
            Some(Ok(file)) => {
                let notice = file.oversized.then(|| {
                    notice::Notice::plain(format!(
                        "{} は 10MB を超えています。動作が重くなることがあります",
                        file.path.display()
                    ))
                });
                let meta = DocumentMeta::opened(file.path, file.format);
                (file.text, meta, notice)
            }
            Some(Err(error)) => (
                String::new(),
                DocumentMeta::untitled(),
                Some(notice::Notice::plain(format!("{error}"))),
            ),
            None => (NEW_DOCUMENT.to_owned(), DocumentMeta::untitled(), None),
        };

        let document = Document::from_text(text);
        // **起動時に作る。** 閉じているなら作らない（rebuild_toc と同じ判断）
        let toc = if settings.toc_visible {
            toc::build(&document)
        } else {
            Vec::new()
        };

        (
            Self {
                document,
                editor: EditorState {
                    caret_visible: true,
                    ..EditorState::default()
                },
                preview: PreviewState::default(),
                mode: if std::env::args().any(|a| a == "--bench-edit-only") {
                    ViewMode::Edit
                } else {
                    match settings.view_mode {
                        crate::io::settings::ViewMode::Edit => ViewMode::Edit,
                        crate::io::settings::ViewMode::Preview => ViewMode::Preview,
                        crate::io::settings::ViewMode::Split => ViewMode::Split,
                    }
                },
                meta,
                pending: None,
                notice,
                queued_answer: None,
                confirming: false,
                settings,
                system_dark: matches!(dark_light::detect(), Ok(dark_light::Mode::Dark)),
                settings_touched: None,
                metrics: Metrics::default(),
                last_edit_ms: 0.0,
                last_edit_full_rescan: false,
                show_ime_log: true,
                // 図は merman + resvg で描く（DEC-207）。
                // 数式と画像は P3 の以降の段階で足す
                embeds: EmbedPool::new(std::sync::Arc::new(
                    Dispatch::new()
                        .with_diagram(DiagramRenderer::new())
                        .with_math(MathRenderer::new())
                        .with_image(ImageRenderer::new()),
                )),
                toc,
                window_width: 1100.0,
                search: search::SearchState::default(),
                export: None,
                export_dialog: None,
                resume_after_save: false,
                picking: false,
                bench: std::env::args()
                    .any(|arg| arg == "--bench-scroll" || arg == "--bench-edit")
                    .then(|| Bench {
                        started: std::time::Instant::now(),
                        ticks: 0,
                        frames: std::cell::Cell::new(0),
                        edit_mode: std::env::args().any(|arg| arg == "--bench-edit"),
                        pending: None,
                        latencies: Vec::new(),
                    }),
            },
            Task::none(),
        )
    }

    /// 目次を作り直す。
    ///
    /// **閉じている間は作らない。** 10MB の文書では全ブロックの走査になるため、
    /// 見えていないものに 1 打鍵ぶんの時間を使わない
    fn rebuild_toc(&mut self) {
        self.toc = if self.settings.toc_visible {
            toc::build(&self.document)
        } else {
            Vec::new()
        };
    }

    /// 未保存の確認（§18.2 / SCR-004）。
    ///
    /// **3 択である。** 保存して続ける / 破棄して続ける / 操作を中止する。
    fn confirm_view(&self) -> Element<'_, Message> {
        let name = self
            .meta
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "無題".to_owned());

        let answer = |label: &'static str, answer: Answer| {
            button(text(label).size(13))
                .padding([6, 16])
                .on_press(Message::Answer(answer))
        };

        container(
            container(
                column![
                    text("保存していない変更があります").size(15),
                    text(format!("{name} の変更を保存しますか。")).size(13),
                    row![
                        answer("保存して続ける", Answer::Save),
                        answer("破棄して続ける", Answer::Discard),
                        answer("中止", Answer::Cancel),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(24),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// 検索バー（§8.2）。
    ///
    /// **表示モードは切り替えない。** 出している間は Enter が次の一致へ、
    /// Shift+Enter が前の一致へ、Esc が閉じるに割り当たる
    fn search_view(&self) -> Element<'_, Message> {
        let step = |label: &'static str, forward: bool| {
            let mut b = button(text(label).size(12)).padding([4, 10]);
            if !self.search.matches.is_empty() {
                b = b.on_press(Message::SearchStep(forward));
            }
            b
        };

        row![
            text_input("検索（原文）", &self.search.query)
                .id(search_input_id())
                .on_input(Message::SearchInput)
                .size(13)
                .width(Length::Fixed(280.0)),
            text(self.search.label()).size(12),
            step("前へ", false),
            step("次へ", true),
            button(text("閉じる").size(12))
                .padding([4, 10])
                .on_press(Message::CloseSearch),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// 出力範囲のダイアログ（SCR-005）。
    ///
    /// **自分で描く。** OS のダイアログは親窓を持てず背面へ回る（§12 / §10.18）
    fn export_dialog_view<'a>(
        &'a self,
        dialog: &'a export_dialog::ExportDialog,
    ) -> Element<'a, Message> {
        let choice = |label: String, kind: RangeKind| {
            let mut b = button(text(label).size(13)).padding([4, 10]);
            if dialog.kind != kind {
                b = b.on_press(Message::SetExportRange(kind));
            }
            b
        };

        let mut panel =
            column![text(format!("{} に出力", dialog.format.label())).size(15)].spacing(10);

        // **HTML は範囲を選ばない**（§17A.2）。ページという単位が無い
        if !dialog.format.takes_range() {
            panel = panel.push(text("文書全体を 1 ファイルに出力します").size(12));
            return self.export_buttons(dialog, panel);
        }

        panel = panel.push(text("出力範囲").size(13)).push(
            row![
                choice("文書全体".to_owned(), RangeKind::All),
                text(dialog.estimate_label()).size(12),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );

        // 見出しを選ぶ（目次から）
        if self.toc.is_empty() {
            panel = panel.push(text("見出しが無いため、見出し単位の指定はできません").size(12));
        } else {
            let items = column(self.toc.iter().enumerate().map(|(index, entry)| {
                let selected = dialog.kind == RangeKind::Heading && dialog.heading == index;
                let indent = f32::from(entry.level.saturating_sub(1)) * 12.0;
                container(
                    button(
                        text(format!(
                            "{} {}",
                            if selected { "●" } else { "○" },
                            entry.title
                        ))
                        .size(12),
                    )
                    .padding([2, 6])
                    .width(Length::Fill)
                    .style(button::text)
                    .on_press(Message::SelectExportHeading(index)),
                )
                .padding(iced::Padding {
                    left: indent,
                    ..iced::Padding::ZERO
                })
                .into()
            }));

            panel = panel.push(choice("見出しを選ぶ".to_owned(), RangeKind::Heading));
            panel = panel.push(
                container(scrollable(items).height(Length::Fixed(140.0)))
                    .width(Length::Fixed(420.0)),
            );
        }

        // ページ指定
        panel = panel.push(
            row![
                choice("ページを指定".to_owned(), RangeKind::Pages),
                text_input("1", &dialog.from)
                    .on_input(Message::SetExportFrom)
                    .size(13)
                    .width(Length::Fixed(70.0)),
                text("ページから").size(12),
                text_input("50", &dialog.count)
                    .on_input(Message::SetExportCount)
                    .size(13)
                    .width(Length::Fixed(70.0)),
                text("ページ").size(12),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );

        self.export_buttons(dialog, panel)
    }

    /// 保存先と、出力・キャンセルのボタン。**形式によらず同じ**。
    fn export_buttons<'a>(
        &'a self,
        dialog: &'a export_dialog::ExportDialog,
        panel: iced::widget::Column<'a, Message>,
    ) -> Element<'a, Message> {
        let mut panel = panel;
        panel = panel.push(
            row![
                text("保存先").size(12),
                text(notice::elide_middle(
                    &dialog.destination.display().to_string(),
                    NOTICE_PATH_CHARS
                ))
                .size(12),
                button(text("参照").size(12))
                    .padding([4, 10])
                    .on_press(Message::BrowseExportDestination),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );

        panel = panel.push(
            row![
                button(text("出力").size(13))
                    .padding([6, 16])
                    .on_press(Message::StartExport),
                button(text("キャンセル").size(13))
                    .padding([6, 16])
                    .on_press(Message::DismissExportDialog),
            ]
            .spacing(8),
        );

        container(container(panel).padding(24))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    /// 目次サイドバー（受入条件 §23.1）。
    ///
    /// **見出しを押すと飛ぶ。** 深さは字下げで示す
    fn toc_view(&self) -> Element<'_, Message> {
        if self.toc.is_empty() {
            return container(text("見出しがありません").size(12))
                .padding(10)
                .into();
        }

        let items = column(self.toc.iter().enumerate().map(|(index, entry)| {
            // 段の深さぶん右へ寄せる。`#` の数がそのまま見た目になる
            let indent = f32::from(entry.level.saturating_sub(1)) * 12.0;
            container(
                button(text(&entry.title).size(12))
                    .padding([2, 6])
                    .width(Length::Fill)
                    .style(button::text)
                    .on_press(Message::JumpTo(index)),
            )
            .padding(iced::Padding {
                left: indent,
                ..iced::Padding::ZERO
            })
            .into()
        }));

        scrollable(items).height(Length::Fill).into()
    }

    pub fn title(&self) -> String {
        format!("{} — Markdown Viewer", self.meta.display_name())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleImeLog => self.show_ime_log = !self.show_ime_log,
            Message::BlinkCaret => {
                self.flush_settings();
                // **届いた図を取り込む。** 取り込むと高さが変わりうるので、
                // 次の描画でレイアウトし直される（§16.5 の置き換え）
                if !self.embeds.poll().is_empty() {
                    self.preview.embeds_pending = self.embeds.pending();
                }
                // 計測中は点滅させない（描画の増減が結果に混ざるため）
                if self.bench.is_none() {
                    self.editor.caret_visible = !self.editor.caret_visible;
                }
            }
            Message::BenchTick => return self.bench_tick(),
            Message::Editor(action) => self.apply(action),
            Message::Preview(action) => self.apply_preview(action),
            Message::SetMode(mode) => {
                self.mode = mode;
                self.settings.view_mode = match mode {
                    ViewMode::Edit => crate::io::settings::ViewMode::Edit,
                    ViewMode::Preview => crate::io::settings::ViewMode::Preview,
                    ViewMode::Split => crate::io::settings::ViewMode::Split,
                };
                self.touch_settings();
                self.sync_from_editor();
            }
            Message::CloseRequested => {
                // **閉じる前に確認する**（§18.2）。確認が要るなら進めない
                if !self.begin(Pending::Exit) {
                    return Task::none();
                }
                return self.run_pending();
            }
            Message::OpenSearch => {
                self.search.open = true;
                // **入力欄へ焦点を移す。** 移さないとキーがエディタへ流れる
                return iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::focus(search_input_id()),
                );
            }
            Message::CloseSearch => {
                self.search.open = false;
                // 強調も消す。閉じたのに色が残ると、何が選ばれているか分からない
                self.search.matches.clear();
                self.search.searching = false;
            }
            Message::SearchInput(query) => {
                self.search.query = query;
                return self.run_search();
            }
            Message::SearchStep(forward) => {
                self.search.advance(forward);
                self.jump_to_match();
            }
            Message::SearchFound { generation, found } => {
                // **古い結果は捨てる。** 打鍵ごとに投げるため追い越しが起きる
                if generation != self.search.generation {
                    return Task::none();
                }
                match found {
                    Ok(found) => {
                        self.search.accept(found);
                        self.jump_to_match();
                    }
                    // **落ちた理由を画面に出す**（§10.32）
                    Err(reason) => {
                        self.search.accept(crate::search::Found::default());
                        self.notice = Some(notice::Notice::plain(reason));
                    }
                }
            }
            Message::ToggleToc => {
                self.settings.toc_visible = !self.settings.toc_visible;
                self.rebuild_toc();
                self.touch_settings();
            }
            Message::TocWidth(delta) => {
                self.settings.toc_width =
                    (self.settings.toc_width + delta).clamp(TOC_MIN_WIDTH, TOC_MAX_WIDTH);
                self.touch_settings();
            }
            Message::SplitRatio(delta) => {
                // **px を比率へ直す。** つまみは px でしか動きを知らない
                let area = self.split_area_width();
                self.settings.split_ratio =
                    (self.settings.split_ratio + delta / area).clamp(SPLIT_MIN, SPLIT_MAX);
                self.touch_settings();
            }
            Message::ToggleSync => {
                self.settings.scroll_sync = !self.settings.scroll_sync;
                self.touch_settings();
                // 入れた瞬間に合わせる。**次にスクロールするまで揃わないのは分かりにくい**
                self.sync_from_editor();
            }
            Message::JumpTo(index) => self.jump_to(index),
            Message::WindowResized(width) => self.window_width = width,
            Message::PickedOpen(path) => {
                self.picking = false;
                let Some(path) = path else {
                    return Task::none();
                };
                return self.load_path(path);
            }
            Message::PickedSave(path) => {
                self.picking = false;
                return self.finish_pick_save(path);
            }
            Message::PickedExport(path, format) => {
                self.picking = false;
                let Some(path) = path else {
                    return Task::none();
                };
                let estimate =
                    crate::export::range::estimate_pages(self.document.text().len_lines());
                self.export_dialog = Some(export_dialog::ExportDialog::new(path, estimate, format));
            }
            Message::PickedExportAgain(path) => {
                self.picking = false;
                if let (Some(path), Some(dialog)) = (path, self.export_dialog.as_mut()) {
                    dialog.destination = path;
                }
            }
            Message::SetTheme(theme) => {
                self.settings.theme = theme;
                self.touch_settings();
            }
            Message::File(command) => return self.start_file_command(command),
            Message::Answer(answer) => return self.answer_confirm(answer),
            Message::Loaded(result) => self.finish_load(result),
            Message::Saved(result) => self.finish_save(result),
            Message::Exported(result) => self.finish_export(result),
            Message::StartExport => return self.export_pdf(),
            Message::DismissExportDialog => self.export_dialog = None,
            Message::SetExportRange(kind) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.kind = kind;
                }
            }
            Message::SelectExportHeading(index) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.heading = index;
                    // 選んだら、その指定に切り替える（押しただけで効くように）
                    dialog.kind = RangeKind::Heading;
                }
            }
            Message::SetExportFrom(text) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.from = text;
                    dialog.kind = RangeKind::Pages;
                }
            }
            Message::SetExportCount(text) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.count = text;
                    dialog.kind = RangeKind::Pages;
                }
            }
            Message::BrowseExportDestination => {
                let format = self
                    .export_dialog
                    .as_ref()
                    .map_or(Format::Pdf, |d| d.format);
                return self
                    .ask(export_picker(&self.meta, format))
                    .map(Message::PickedExportAgain);
            }
            Message::CancelExport => {
                // **止めるのはワーカー。** ここでは印を立てるだけで、
                // 後始末は出力側が各ページで見て行う
                if let Some(job) = &self.export {
                    job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Message::DismissNotice => self.notice = None,
            Message::OpenNoticeLink => self.open_notice_link(),
        }
        Task::none()
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Move(movement) => {
                cursor::move_cursor(&self.document, &mut self.editor, movement)
            }
            Action::Scrolled { lines } => {
                // **端数を繰り越す。** 1 回ぶんを丸めると、高解像度ホイールで
                // ゆっくり回したときに 1 行も動かない（§10.10）
                let step = crate::render::take_whole_lines(&mut self.editor.scroll_carry, lines);
                if step != 0 {
                    let total = self.document.text().len_lines();
                    self.editor.top_line = (self.editor.top_line as i64 + step)
                        .clamp(0, total.saturating_sub(1) as i64)
                        as usize;
                    self.sync_from_editor();
                }
            }
            // **上限は描画層が測って添えてくる**（字幅を知るのはあちらだけ）
            Action::ScrolledX { delta, max } => {
                self.editor.scroll_x =
                    crate::render::advance_scroll_x(self.editor.scroll_x, delta, max);
            }
            Action::ScrollXTo { to } => self.editor.scroll_x = to.max(0.0),
            Action::Insert(text) => self.insert(&text),
            Action::Backspace => self.backspace(),
            Action::Ime(ime) => self.apply_ime(ime),
        }
    }

    /// プレビューからの通知（§3.7）。
    fn apply_preview(&mut self, action: PreviewAction) {
        match action {
            PreviewAction::Scrolled(delta) => {
                // ここでだけアンカーを作り直す
                let viewport = 600.0;
                self.preview.anchor =
                    self.preview
                        .anchor
                        .scrolled(delta, self.document.heights(), viewport);
                self.sync_from_preview();
            }
            PreviewAction::Measured {
                embed_requests,
                updates,
                laid_out,
                hit_rate,
            } => {
                // **依頼はここで投げる。** 同じ鍵は二重に投げられない
                for request in embed_requests {
                    self.embeds.request(request);
                }
                self.preview.embeds_pending = self.embeds.pending();

                // **アンカーは変えない。** 高さだけを直す。
                // ここで作り直すと、上方の高さが確定したときに内容が飛ぶ（§3.7）
                self.preview.measured = updates.len();
                self.preview.laid_out = laid_out;
                self.preview.hit_rate = hit_rate;
                self.document.set_heights(&updates);

                // **ここが「反映が終わった」時点である**（OPEN-201）。
                // 可視範囲のレイアウトが済み、高さが索引へ戻ったところ
                if let Some(bench) = self.bench.as_mut() {
                    if let Some(started) = bench.pending.take() {
                        bench
                            .latencies
                            .push(started.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
        }
    }

    /// IME の状態遷移（§4.5）。
    fn apply_ime(&mut self, ime: ImeAction) {
        self.log_ime(&ime);

        match ime {
            // **未確定文字列はロープに入れない。** 描画のためだけに持つ
            ImeAction::Preedit(content) => {
                self.editor.preedit = if content.is_empty() {
                    None
                } else {
                    Some(content)
                };
            }
            // 確定したら 1 回の編集として適用する
            ImeAction::Commit(text) => {
                self.editor.preedit = None;
                self.insert(&text);
            }
            ImeAction::Opened => {}
            ImeAction::Closed => self.editor.preedit = None,
        }
    }

    fn log_ime(&mut self, ime: &ImeAction) {
        let note = match ime {
            ImeAction::Opened => "Opened".to_owned(),
            ImeAction::Closed => "Closed".to_owned(),
            ImeAction::Preedit(content) => format!("Preedit {content:?}"),
            ImeAction::Commit(text) => format!("Commit {text:?}"),
        };
        self.editor.ime_log.push(note);
        if self.editor.ime_log.len() > 20 {
            self.editor.ime_log.remove(0);
        }
    }

    /// カーソル位置へ文字列を挿入する。
    fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = self
            .document
            .byte_at(self.editor.cursor_line, self.editor.cursor_column);

        let started = std::time::Instant::now();
        let outcome = self
            .document
            .edit(at..at, text, self.width(), &self.metrics);
        self.last_edit_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.last_edit_full_rescan = outcome.full_rescan;

        let (line, column) = self.document.position_at(at + text.len());
        self.editor.cursor_line = line;
        self.editor.cursor_column = column;
        self.editor.goal_column = None;
        self.meta.dirty = true;
        self.rebuild_toc();
        self.follow_cursor();
    }

    /// カーソルの直前の 1 文字を消す。
    fn backspace(&mut self) {
        let at = self
            .document
            .byte_at(self.editor.cursor_line, self.editor.cursor_column);
        if at == 0 {
            return;
        }
        // **文字境界へ下ろす。** 日本語は 1 文字が複数バイトのため
        let (prev_line, prev_column) = if self.editor.cursor_column > 0 {
            (self.editor.cursor_line, self.editor.cursor_column - 1)
        } else if self.editor.cursor_line > 0 {
            let line = self.editor.cursor_line - 1;
            (line, usize::MAX)
        } else {
            return;
        };
        let from = self.document.byte_at(prev_line, prev_column);

        let started = std::time::Instant::now();
        let outcome = self
            .document
            .edit(from..at, "", self.width(), &self.metrics);
        self.last_edit_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.last_edit_full_rescan = outcome.full_rescan;

        let (line, column) = self.document.position_at(from);
        self.editor.cursor_line = line;
        self.editor.cursor_column = column;
        self.meta.dirty = true;
        self.rebuild_toc();
        self.follow_cursor();
    }

    /// 分割されている側の幅（px）。つまみの換算に使う。
    fn split_area_width(&self) -> f32 {
        let toc = if self.settings.toc_visible {
            self.settings.toc_width + crate::render::DIVIDER_WIDTH
        } else {
            0.0
        };
        (self.window_width - toc - crate::render::DIVIDER_WIDTH).max(MIN_SPLIT_AREA)
    }

    /// 同期が効く場面か（§8 / §16.14）。
    ///
    /// **分割表示のときだけ効く。** 片方しか見えていないなら合わせる相手がいない
    fn syncing(&self) -> bool {
        self.settings.scroll_sync && self.mode == ViewMode::Split
    }

    /// エディタの位置をプレビューへ移す。
    ///
    /// **戻りの同期は起きない。** 合わせる側の状態を直接書いており、
    /// メッセージを投げ返さないため、§16.14 が警告するループにならない
    fn sync_from_editor(&mut self) {
        if !self.syncing() {
            return;
        }
        self.preview.anchor = sync::to_preview(&self.document, self.editor.top_line);
    }

    /// プレビューの位置をエディタへ移す。
    fn sync_from_preview(&mut self) {
        if !self.syncing() {
            return;
        }
        self.editor.top_line = sync::to_editor(&self.document, self.preview.anchor);
    }

    /// 走査をワーカーへ投げる（§15.5）。
    ///
    /// **UI の糸では走査しない。** 10MB を舐めるため、打鍵のたびに止まる
    fn run_search(&mut self) -> Task<Message> {
        self.search.generation += 1;
        let generation = self.search.generation;

        if self.search.query.is_empty() {
            self.search.accept(crate::search::Found::default());
            return Task::none();
        }

        self.search.searching = true;
        // **ロープの複製は安い。** 木を共有するので 10MB を写さない
        let rope = self.document.text().clone();
        let query = self.search.query.clone();
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            // 走査が落ちても理由を返す（§10.32）
            let found =
                crate::worker::catch("検索", move || crate::search::find_all(&rope, &query));
            let _ = sender.send(found);
        });

        Task::perform(
            async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err("検索が中断されました".to_owned()))
            },
            move |found| Message::SearchFound { generation, found },
        )
    }

    /// いま選んでいる一致を画面に出す。
    fn jump_to_match(&mut self) {
        let Some(found) = self.search.current_match() else {
            return;
        };
        let (line, column) = self.document.position_at(found.start);
        self.editor.cursor_line = line;
        self.editor.cursor_column = column;
        self.editor.goal_column = None;
        self.editor.scroll_carry = 0.0;
        // **少し上に余白を残す。** 画面の一番上だと前後が読めない
        self.editor.top_line = line.saturating_sub(3);

        // プレビューも寄せる。**同期の設定によらない**（探して飛ぶのは明示の操作）
        if self.mode != ViewMode::Edit {
            self.preview.anchor = sync::to_preview(&self.document, line);
        }
    }

    /// 目次の項目へ飛ぶ（受入条件 §23.1）。
    ///
    /// **エディタとプレビューの両方を動かす。** 分割表示のとき、
    /// 片側だけが動くと目次で選んだ見出しが反対側に出てこない
    fn jump_to(&mut self, index: usize) {
        let Some(entry) = self.toc.get(index) else {
            return;
        };
        let (line, block_id) = (entry.line, entry.block_id);

        self.editor.top_line = line;
        self.editor.cursor_line = line;
        self.editor.cursor_column = 0;
        self.editor.goal_column = None;
        self.editor.scroll_carry = 0.0;

        // 見出しブロックの先頭を画面の一番上にする
        self.preview.anchor = ScrollAnchor {
            block_id,
            offset_in_block: 0.0,
        };
    }

    /// レイアウト幅。P1 ではウィンドウ幅を追えないため固定値を使う。
    fn width(&self) -> f32 {
        1100.0
    }

    /// ファイル操作を始める（§14.1 / §18.2）。
    ///
    /// **未保存の確認はここに 1 か所だけ置く。** 操作ごとに書くと忘れる。
    fn start_file_command(&mut self, command: FileCommand) -> Task<Message> {
        match command {
            FileCommand::ExportPdf => return self.open_export_dialog(Format::Pdf),
            FileCommand::ExportHtml => return self.open_export_dialog(Format::Html),
            FileCommand::Save => return self.save_now(needs_save_as(&self.meta)),
            FileCommand::SaveAs => return self.save_now(true),
            // 確認を出した場合は、答えが来るまで進めない
            FileCommand::New => {
                if !self.begin(Pending::New) {
                    return Task::none();
                }
            }
            FileCommand::Open => {
                if !self.begin(Pending::Open) {
                    return Task::none();
                }
            }
        }
        self.run_pending()
    }

    /// 内容を失いうる操作を始める。未保存なら確認を挟む。
    ///
    /// **確認はアプリ内に描く**（§12 / SCR-004）。
    /// OS のメッセージダイアログを使うと、親ウィンドウを渡さない限り
    /// アプリの背面に出ることがあり、**本体が固まったように見える**
    /// （実際に踏んだ）。ファイル選択だけは OS のものを使う。
    fn begin(&mut self, action: Pending) -> bool {
        match decide(&self.meta, action) {
            Next::Proceed(action) => {
                self.pending = Some(action);
                true
            }
            Next::Confirm(action) => {
                self.pending = Some(action);
                self.confirming = true;
                false
            }
        }
    }

    fn apply_answer(&mut self, answer: Answer) {
        if answer == Answer::Cancel {
            self.pending = None;
        }
        self.queued_answer = Some(answer);
    }

    fn answer_confirm(&mut self, answer: Answer) -> Task<Message> {
        self.confirming = false;
        self.apply_answer(answer);
        self.run_pending()
    }

    /// 確認の答えに従って、保留していた操作を進める。
    fn run_pending(&mut self) -> Task<Message> {
        match self.queued_answer.take() {
            Some(Answer::Cancel) => return Task::none(),
            // **保存してから続ける。** 保存に失敗したら続けない
            Some(Answer::Save) => {
                if needs_save_as(&self.meta) {
                    // **保存先を聞いてから続ける。** 聞いている間は保留を持ったまま待つ
                    self.resume_after_save = true;
                    return self.save_now(true);
                }
                let task = self.save_now(false);
                if self.meta.dirty {
                    // 保存できなかった
                    self.pending = None;
                    return task;
                }
            }
            Some(Answer::Discard) | None => {}
        }

        self.finish_pending()
    }

    /// 保留していた操作を実行する（保存の判断が済んだあと）。
    fn finish_pending(&mut self) -> Task<Message> {
        let Some(action) = self.pending.take() else {
            return Task::none();
        };
        match action {
            Pending::New => {
                self.replace_document(NEW_DOCUMENT.to_owned(), DocumentMeta::untitled());
                Task::none()
            }
            Pending::Open => self.ask(picker::Picker::open()).map(Message::PickedOpen),
            Pending::OpenPath(path) => self.load_path(path),
            Pending::Exit => {
                // **終了前に設定を書く。** デバウンスの途中で終わると消える
                self.settings_touched =
                    Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
                self.flush_settings();
                iced::exit()
            }
        }
    }

    /// 読み込みを走らせる。**UI スレッドで読まない**（遅い共有ドライブを考慮。§4.3）。
    fn load_path(&mut self, path: std::path::PathBuf) -> Task<Message> {
        Task::perform(
            async move {
                crate::io::load(&path)
                    .map(Box::new)
                    .map_err(|e| e.to_string())
            },
            Message::Loaded,
        )
    }

    fn finish_load(&mut self, result: Result<Box<crate::io::LoadedFile>, String>) {
        match result {
            Ok(file) => {
                if file.oversized {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} は 10MB を超えています。動作が重くなることがあります",
                        file.path.display()
                    )));
                } else {
                    self.notice = None;
                }
                let meta = DocumentMeta::opened(file.path.clone(), file.format.clone());
                self.replace_document(file.text.clone(), meta);
            }
            Err(reason) => self.notice = Some(notice::Notice::plain(reason)),
        }
    }

    /// 文書を差し替える。**状態を残さない。**
    ///
    /// カーソル・スクロール・キャッシュを持ち越すと、前の文書の位置に
    /// 飛んだり古い図が出たりする。
    fn replace_document(&mut self, text: String, meta: DocumentMeta) {
        self.document = Document::from_text(text);
        self.editor = EditorState {
            caret_visible: true,
            ..EditorState::default()
        };
        self.preview = PreviewState::default();
        self.meta = meta;
        self.last_edit_ms = 0.0;
        self.last_edit_full_rescan = false;
        self.rebuild_toc();
    }

    /// 保存する。`ask` なら保存先を聞く。
    ///
    /// **聞く場合は待たない。** ダイアログは親を渡すために `Task` 越しに出すので、
    /// 結果は `Message::PickedSave` で戻ってくる
    fn save_now(&mut self, ask: bool) -> Task<Message> {
        if ask {
            return self.ask(save_picker(&self.meta)).map(Message::PickedSave);
        }
        let Some(path) = self.meta.path.clone() else {
            return Task::none();
        };
        self.save_to(path)
    }

    /// 指定の場所へ書く。
    fn save_to(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let text = self.document.text().to_string();
        let format = self.meta.format.clone();

        // **成功を先に反映しない。** 書けたことを確かめてから未保存を下ろす
        match crate::io::save(&path, &text, &format) {
            Ok(()) => {
                self.meta.path = Some(path.clone());
                self.meta.dirty = false;
                self.notice = None;
            }
            Err(error) => self.notice = Some(notice::Notice::plain(format!("{error}"))),
        }
        Task::none()
    }

    fn finish_save(&mut self, result: Result<std::path::PathBuf, String>) {
        match result {
            Ok(path) => {
                self.meta.path = Some(path);
                self.meta.dirty = false;
            }
            Err(reason) => self.notice = Some(notice::Notice::plain(reason)),
        }
    }

    /// PDF に出力する（§17）。
    ///
    /// **未保存の確認は挟まない。** 出力は文書を変えないため、
    /// 保存していなくても、いま見えているものをそのまま出せばよい
    /// ダイアログを出す。**親を渡すために窓の取っ手を借りる**。
    ///
    /// 取っ手は iced の `Task` 越しにしか取れないので、選んだ結果は
    /// メッセージで戻ってくる（同期では受け取れない）
    fn ask(&mut self, picker: picker::Picker) -> Task<Option<std::path::PathBuf>> {
        self.picking = true;
        // `and_then` は窓が無ければ何も流さない。**親無しでは出さない**
        // （親が無いとダイアログが背面へ回る。§10.29）
        iced::window::latest()
            .and_then(move |id| {
                let picker = picker.clone();
                // 取っ手を借りるのは主の糸。**出すのは別の糸**（§10.31）
                iced::window::run(id, move |handle| picker.show(handle))
            })
            .then(Task::future)
    }

    /// 出力範囲のダイアログを開く（SCR-005）。
    ///
    /// **いきなり出力しない。** 10MB の文書は約 9,400 ページになるため、
    /// どこを出すのかを選ばせる（§17.11）
    fn open_export_dialog(&mut self, format: Format) -> Task<Message> {
        if self.export.is_some() {
            return Task::none();
        }
        self.ask(export_picker(&self.meta, format))
            .map(move |path| Message::PickedExport(path, format))
    }

    /// 保存先が決まったあとの処理。
    ///
    /// **保留していた操作があれば、書けたときだけ続ける**（§18.2）。
    fn finish_pick_save(&mut self, path: Option<std::path::PathBuf>) -> Task<Message> {
        let resume = std::mem::take(&mut self.resume_after_save);

        let Some(path) = path else {
            // 取り消した。保留していた操作もやめる（保存せずには進めない）
            if resume {
                self.pending = None;
            }
            return Task::none();
        };

        let task = self.save_to(path);
        if !resume {
            return task;
        }
        if self.meta.dirty {
            // 書けなかった。**続けない**
            self.pending = None;
            return task;
        }
        Task::batch([task, self.finish_pending()])
    }

    fn export_pdf(&mut self) -> Task<Message> {
        // **二重に走らせない。** 同じ先へ 2 つが書き込む
        if self.export.is_some() {
            return Task::none();
        }
        let Some(dialog) = self.export_dialog.take() else {
            return Task::none();
        };
        let path = dialog.destination.clone();
        // 目次の項目をブロック添字へ直す（見出し単位の指定に使う）
        let headings: Vec<usize> = self.toc.iter().map(|entry| entry.block_id).collect();
        let chosen = dialog.range(&headings);
        let format = dialog.format;

        let job = ExportJob::new();
        let watcher = job.watch();
        self.export = Some(job);

        // **その時点の文書を写して渡す**（§17.10）。写さずに借りると、
        // 出力中は編集できなくなる。ロープは木を共有するので複写は軽い
        let document = self.document.clone();
        let title = self.meta.display_title();
        let base_dir = self.meta.base_dir().map(std::path::Path::to_path_buf);
        let destination = path.clone();
        let selection = chosen;

        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            // **落ちても理由を返す**（§10.32）。返さないと「中断されました」
            // としか出ず、何が起きたのか画面から分からない
            let outcome = crate::worker::catch("出力", move || {
                // **画面と同じ測り方をする**（§17.3）。書体も大きさも同じ規則で選ぶ
                let measurer = crate::render::PdfMeasurer::new(crate::export::pdf::BODY_SIZE);
                let result = match format {
                    Format::Pdf => crate::export::export_pdf(
                        &document,
                        &measurer,
                        &title,
                        base_dir.as_deref(),
                        &watcher,
                        selection,
                    )
                    .map(|bytes| (bytes, 0)),
                    // **HTML は範囲を取らない。** ページという単位が無く、
                    // 1 ファイルで完結させるのが目的だからである（§17A.2）
                    Format::Html => {
                        crate::export::export_html(&document, &title, base_dir.as_deref(), &watcher)
                            .map(|html| (html.text.into_bytes(), html.skipped_images))
                    }
                }
                .map_err(|error| format!("{error}"))
                // **書き出すのは最後だけ。** 途中で書くと、取り消したときに
                // 書きかけのファイルが残る（§17.10）
                .and_then(|(bytes, skipped)| {
                    std::fs::write(&destination, bytes)
                        .map(|()| (destination.clone(), skipped))
                        .map_err(|error| {
                            format!("{} に書き出せません: {error}", destination.display())
                        })
                });
                result
            });
            let _ = sender.send(outcome.unwrap_or_else(Err));
        });

        Task::perform(
            async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err("出力が中断されました".to_owned()))
            },
            Message::Exported,
        )
    }

    fn finish_export(&mut self, result: Result<(std::path::PathBuf, usize), String>) {
        self.export = None;
        self.notice = Some(match result {
            // **その場から開けるようにする。** 出力して終わりではなく、
            // たいていは中身を確かめたい
            Ok((path, skipped)) => {
                // 埋め込めなかった画像は件数で知らせる（§17A.5）
                let text = if skipped > 0 {
                    format!("出力しました（埋め込めない画像が {skipped} 件）")
                } else {
                    "出力しました".to_owned()
                };
                notice::Notice::file(text, path)
            }
            Err(reason) => notice::Notice::plain(reason),
        });
    }

    /// 知らせに出ているファイルを OS の既定のアプリで開く。
    fn open_notice_link(&mut self) {
        let Some(path) = self.notice.as_ref().and_then(|notice| notice.link.clone()) else {
            return;
        };
        // **開けなかったことも知らせる。** 黙って何も起きないのが一番困る
        if let Err(error) = crate::io::launch::open(&path) {
            self.notice = Some(notice::Notice::plain(format!(
                "{} を開けません: {error}",
                path.display()
            )));
        }
    }

    /// 設定が変わった。**すぐには書かない**（§13.5）。
    ///
    /// ズームやパネル幅は連続して変わるため、変更のたびに書くと
    /// ディスクを叩き続けることになる。
    fn touch_settings(&mut self) {
        self.settings_touched = Some(std::time::Instant::now());
    }

    /// 溜まった設定の変更を書く。**変更から 1 秒**（§13.5）。
    fn flush_settings(&mut self) {
        let Some(touched) = self.settings_touched else {
            return;
        };
        if touched.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.settings_touched = None;
        // **書けなくても動き続ける。** 設定は保存できなくても致命ではない
        if let Err(reason) = self.settings.save() {
            self.notice = Some(notice::Notice::plain(format!(
                "設定を保存できません: {reason}"
            )));
        }
    }

    /// いま使う外観（§10.4）。
    pub fn theme(&self) -> iced::Theme {
        match self.settings.theme {
            ThemePreference::Light => iced::Theme::Light,
            ThemePreference::Dark => iced::Theme::Dark,
            ThemePreference::System => {
                if self.system_dark {
                    iced::Theme::Dark
                } else {
                    iced::Theme::Light
                }
            }
        }
    }

    /// カーソルが画面外へ出たらスクロールする。
    fn follow_cursor(&mut self) {
        // 可視行数は描画時にしか分からないため、ここでは概算で寄せる
        const ROUGH_ROWS: usize = 30;
        if self.editor.cursor_line < self.editor.top_line {
            self.editor.top_line = self.editor.cursor_line;
        } else if self.editor.cursor_line >= self.editor.top_line + ROUGH_ROWS {
            self.editor.top_line = self.editor.cursor_line + 1 - ROUGH_ROWS;
        }
    }

    /// スクロール計測を 1 フレーム進める。
    fn bench_tick(&mut self) -> Task<Message> {
        let total = self.document.text().len_lines();
        let max_top = total.saturating_sub(44);

        let Some(bench) = self.bench.as_mut() else {
            return Task::none();
        };
        bench.ticks += 1;

        let elapsed = bench.started.elapsed().as_secs_f64();
        if elapsed >= BENCH_SECS {
            let frames = bench.frames.get();
            let (median, worst, count) = latency_summary(&mut bench.latencies);
            println!(
                "BENCHRESULT {{\"lines\":{total},\"ticks\":{},\"frames\":{frames},                 \"elapsed_s\":{elapsed:.2},\"fps\":{:.1},\"tick_ms\":{},\"gutter\":{},                 \"edits\":{count},\"latency_median_ms\":{median:.2},\"latency_worst_ms\":{worst:.2}}}",
                bench.ticks,
                frames as f64 / elapsed,
                bench_tick_ms(),
                bench_show_gutter()
            );
            return iced::exit();
        }

        if bench.edit_mode {
            // **前の打鍵の反映を待ってから次を打つ。** 待たずに打つと
            // レイテンシではなく処理の滞留を測ることになる
            if bench.pending.is_some() {
                return Task::none();
            }
            bench.pending = Some(std::time::Instant::now());
            self.bench_edit();
            return Task::none();
        }

        self.editor.top_line += BENCH_LINES_PER_FRAME;
        if self.editor.top_line > max_top {
            self.editor.top_line = 0;
        }

        // プレビューも同時に送る。**レイアウトの負荷を含めて測る**ため
        let heights = self.document.heights();
        let step = 7.0 * 24.0;
        self.preview.anchor = self.preview.anchor.scrolled(step, heights, 600.0);

        Task::none()
    }

    /// 計測用の 1 打鍵。**文書の中ほど**へ 1 文字入れる（§12.8 の通常ケース）。
    fn bench_edit(&mut self) {
        if self.editor.cursor_line == 0 {
            let middle = self.document.text().len_lines() / 2;
            self.editor.cursor_line = middle;
            self.editor.cursor_column = 0;
            self.editor.top_line = middle.saturating_sub(20);
            // プレビューも同じあたりを見る。**編集箇所が可視範囲に入っていないと
            // 再レイアウトが起きず、測りたいものを測れない**
            let at = self.document.byte_at(middle, 0);
            if let Some(block_id) = self.document.block_at_byte(at) {
                self.preview.anchor = ScrollAnchor {
                    block_id,
                    offset_in_block: 0.0,
                };
            }
        }
        self.insert("あ");
    }

    /// キャレットの点滅と、計測中の駆動。
    pub fn subscription(&self) -> iced::Subscription<Message> {
        if self.bench.is_some() {
            // vsync（約 60Hz）に合わせる。PoC と同じ駆動間隔
            // **実フレーム駆動で測る。**
            // タイマー駆動だと、刻みと vsync の食い違いがフレーム数に混ざる。
            // `BENCH_TICK_MS` を指定したときだけタイマー駆動にする（切り分け用）。
            return if std::env::var("BENCH_TICK_MS").is_ok() {
                iced::time::every(std::time::Duration::from_millis(bench_tick_ms()))
                    .map(|_| Message::BenchTick)
            } else {
                iced::window::frames().map(|_| Message::BenchTick)
            };
        }
        // **閉じる要求を自分で受ける。** 既定では確認する間もなく終了する（§18.2）
        iced::Subscription::batch([
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::BlinkCaret),
            iced::window::close_requests().map(|_| Message::CloseRequested),
            // つまみは px でしか動かないため、比率へ直すのに窓幅が要る
            iced::window::resize_events().map(|(_, size)| Message::WindowResized(size.width)),
            // `Ctrl + F` で検索（§8.1 のキー割り当て）。
            //
            // **捕まった出来事も見る。** 焦点のある入力欄は `Esc` を捕まえるため、
            // 捕まっていないものだけを見る購読では検索バーを閉じられない
            iced::event::listen_with(|event, _status, _window| {
                let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    ref key,
                    modifiers,
                    ..
                }) = event
                else {
                    return None;
                };
                (modifiers.command() && key.as_ref() == iced::keyboard::Key::Character("f"))
                    .then_some(Message::OpenSearch)
            }),
            // **検索バーを出している間だけ効かせる。** 常時だと本文の Enter を奪う
            if self.search.open {
                iced::event::listen_with(|event, _status, _window| {
                    use iced::keyboard::key::Named;
                    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        ref key,
                        modifiers,
                        ..
                    }) = event
                    else {
                        return None;
                    };
                    match key.as_ref() {
                        iced::keyboard::Key::Named(Named::Escape) => Some(Message::CloseSearch),
                        iced::keyboard::Key::Named(Named::Enter) => {
                            Some(Message::SearchStep(!modifiers.shift()))
                        }
                        _ => None,
                    }
                })
            } else {
                iced::Subscription::none()
            },
        ])
    }

    pub fn view(&self) -> Element<'_, Message> {
        // 初回描画の時刻を 1 度だけ出す（DD-OPEN-01）
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            if let Some(start) = crate::PROCESS_START.get() {
                eprintln!(
                    "STARTUP 起動〜初回描画 {:.0}ms",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
        });

        // 実描画回数を数える。**タイマーの刻み数とは一致しない**ため別に数える
        // （PoC で 1 度取り違え、vsync を超える値が出た。§6.1）
        if let Some(bench) = &self.bench {
            bench.frames.set(bench.frames.get() + 1);
        }

        let mut editor = EditorView::new(&self.document, &self.editor, Message::Editor)
            .highlight(&self.search.matches, self.search.current_match())
            // **検索バーへ入力している間は本文へ入れない**
            .accept_keys(!self.search.open);
        if self.bench.is_some() {
            editor = editor.show_gutter(bench_show_gutter());
        }
        let editor = container(editor).width(Length::Fill).height(Length::Fill);
        let preview = container(
            PreviewView::new(&self.document, &self.preview, Message::Preview)
                .embeds(&self.embeds)
                .base_dir(self.meta.base_dir())
                .highlight(if self.search.open {
                    self.search.query.as_str()
                } else {
                    ""
                }),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        // 表示モード（§8）
        let main: Element<'_, Message> = match self.mode {
            ViewMode::Edit => editor.into(),
            ViewMode::Preview => preview.into(),
            ViewMode::Split => {
                // 設定の比率で分ける（§13.5）。つまみで変える口は次の段階
                let left = (self.settings.split_ratio * 100.0).round().max(1.0) as u16;
                let right = (100 - left.min(99)).max(1);
                row![
                    container(editor).width(Length::FillPortion(left)),
                    // **掴んで動かせる**（受入条件 §23.1）
                    Divider::new(Message::SplitRatio),
                    container(preview).width(Length::FillPortion(right)),
                ]
                .into()
            }
        };

        // **確認中は本文を覆う**（§18.2 / SCR-004）。
        // 答えるまで編集させないことで、状態の食い違いを防ぐ
        let main: Element<'_, Message> = if self.confirming {
            self.confirm_view()
        } else if let Some(dialog) = &self.export_dialog {
            // **答えるまで本文を触らせない**（確認ダイアログと同じ扱い）
            self.export_dialog_view(dialog)
        } else if self.settings.toc_visible {
            row![
                container(self.toc_view())
                    .width(Length::Fixed(self.settings.toc_width))
                    .height(Length::Fill),
                Divider::new(Message::TocWidth),
                main,
            ]
            .into()
        } else {
            main
        };

        let mode_button = |label: &'static str, mode: ViewMode| {
            let mut b = button(text(label).size(12)).padding([4, 10]);
            if self.mode != mode {
                b = b.on_press(Message::SetMode(mode));
            }
            b
        };
        let file_button = |label: &'static str, command: FileCommand| {
            let mut b = button(text(label).size(12)).padding([4, 10]);
            // **選んでいる最中は押させない**（2 つ開けてしまう。§10.31）
            if !self.picking {
                b = b.on_press(Message::File(command));
            }
            b
        };

        let theme_button = |label: &'static str, preference: ThemePreference| {
            let mut b = button(text(label).size(12)).padding([4, 8]);
            if self.settings.theme != preference {
                b = b.on_press(Message::SetTheme(preference));
            }
            b
        };

        let toolbar = row![
            file_button("New", FileCommand::New),
            file_button("Open", FileCommand::Open),
            file_button("Save", FileCommand::Save),
            file_button("Save As", FileCommand::SaveAs),
            {
                let mut b = button(text("PDF").size(12)).padding([4, 10]);
                if self.export.is_none() && !self.picking {
                    b = b.on_press(Message::File(FileCommand::ExportPdf));
                }
                b
            },
            {
                let mut b = button(text("HTML").size(12)).padding([4, 10]);
                if self.export.is_none() && !self.picking {
                    b = b.on_press(Message::File(FileCommand::ExportHtml));
                }
                b
            },
            button(text("検索").size(12))
                .padding([4, 10])
                .on_press(Message::OpenSearch),
            text("  ").size(12),
            mode_button("Edit", ViewMode::Edit),
            mode_button("Preview", ViewMode::Preview),
            mode_button("Split", ViewMode::Split),
            text("  ").size(12),
            {
                let mut b = button(
                    text(if self.settings.scroll_sync {
                        "Sync ON"
                    } else {
                        "Sync OFF"
                    })
                    .size(12),
                )
                .padding([4, 10]);
                // **Split のときだけ効く**（§12 の一覧）
                if self.mode == ViewMode::Split {
                    b = b.on_press(Message::ToggleSync);
                }
                b
            },
            text("  ").size(12),
            button(
                text(if self.settings.toc_visible {
                    "TOC 非表示"
                } else {
                    "TOC 表示"
                })
                .size(12)
            )
            .padding([4, 10])
            .on_press(Message::ToggleToc),
            text("  ").size(12),
            theme_button("Light", ThemePreference::Light),
            theme_button("Dark", ThemePreference::Dark),
            theme_button("System", ThemePreference::System),
        ]
        .spacing(6);

        let status = text(format!(
            "{} 行 / {} ブロック / 見出し {} 件   Ln {}, Col {}   編集 {:.2}ms{}                プレビュー: {} ブロック / {} 件実測 / 命中 {:.0}%{}",
            self.document.text().len_lines(),
            self.document.blocks().len(),
            self.document.headings().count(),
            self.editor.cursor_line + 1,
            self.editor.cursor_column + 1,
            self.last_edit_ms,
            if self.last_edit_full_rescan {
                "（全走査）"
            } else {
                ""
            },
            self.preview.laid_out,
            self.preview.measured,
            self.preview.hit_rate * 100.0,
            match &self.editor.preedit {
                Some(preedit) => format!("   [変換中: {preedit}]"),
                None => String::new(),
            }
        ))
        .size(12);

        let mut body = column![container(toolbar).padding(6)];

        // 検索バー（§8.2）
        if self.search.open {
            body = body.push(container(self.search_view()).padding([0, 6]));
        }

        // 出力の進み具合（§17.10）。**取り消せる**
        if let Some(job) = &self.export {
            body = body.push(
                container(
                    row![
                        text(job.label()).size(12),
                        button(text("取り消し").size(11))
                            .padding([2, 8])
                            .on_press(Message::CancelExport),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding(6),
            );
        }

        // **失敗は画面に出す。** 握りつぶすと利用者は何が起きたか分からない
        if let Some(notice) = &self.notice {
            let mut line = row![text(&notice.text).size(12)].spacing(8);

            // 出力したファイルは、その場から開けるようにする。
            // **長いパスは真ん中を省く**（窓の幅を押し広げないため）
            if let Some(label) = notice.link_label(NOTICE_PATH_CHARS) {
                line = line.push(
                    button(text(label).size(12))
                        .padding([2, 4])
                        .style(button::text)
                        .on_press(Message::OpenNoticeLink),
                );
            }

            body = body.push(
                container(
                    line.push(
                        button(text("閉じる").size(11))
                            .padding([2, 8])
                            .on_press(Message::DismissNotice),
                    )
                    .align_y(iced::Alignment::Center),
                )
                .padding(6),
            );
        }

        let mut body = body.push(main).push(container(status).padding(6));

        if self.show_ime_log {
            let entries = column(self.editor.ime_log.iter().map(|entry| {
                text(entry)
                    .size(11)
                    .font(crate::render::fonts::mono())
                    .into()
            }));
            body = body.push(
                container(
                    column![
                        text("受信した IME イベント（P1 の確認用）").size(11),
                        scrollable(entries).height(Length::Fixed(90.0)),
                    ]
                    .spacing(4),
                )
                .padding(6),
            );
        }

        body.into()
    }
}
