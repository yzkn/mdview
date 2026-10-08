//! 移動（v2.1.0 R-07 / R-18 / R-19 / R-20）。
//!
//! **どこへ行くかの判定は `edit::navigate` が持つ。** ここは文書から行を
//! 集め、結果に従ってキャレットと画面を動かす。

use std::collections::{BTreeSet, HashMap, HashSet};

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use super::{fold, notice, sync, App};
use crate::edit::navigate::{self, LinkKind, Seek, Target};
use crate::parse::BlockKind;
use crate::render::{Message, ViewMode};

/// 一覧の 1 件（参照の一覧・リンク切れの一覧）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultItem {
    pub line: usize,
    /// 行の中の字の位置
    pub column: usize,
    pub label: String,
}

/// 画面の下に出す一覧。
#[derive(Debug, Clone, Default)]
pub struct Results {
    pub title: String,
    pub items: Vec<ResultItem>,
}

/// 見出しを絞り込んで選ぶ欄（R-18）。
#[derive(Debug, Clone, Default)]
pub struct HeadingPicker {
    pub query: String,
}

/// 見出し 1 つ（移動に使う）。
#[derive(Debug, Clone)]
pub struct HeadingEntry {
    pub line: usize,
    pub level: u8,
    pub title: String,
    pub block_id: usize,
}

/// 一覧に並べる数の上限。**10MB の文書で数万件を並べない**
const MAX_RESULTS: usize = 500;
/// 見出しの絞り込みに並べる数
const MAX_PICKER: usize = 200;

/// 絞り込み（大文字小文字を区別しない、部分一致）。
pub fn matches_filter(title: &str, query: &str) -> bool {
    let query = query.trim();
    query.is_empty() || title.to_lowercase().contains(&query.to_lowercase())
}

impl App {
    // --- 文書から集める ---

    /// 見出しの一覧（目次と違い、上限を設けない）。
    pub(super) fn heading_entries(&self) -> Vec<HeadingEntry> {
        self.document
            .headings()
            .map(|(block_id, level, block)| {
                let first = self.line_text(block.start_line);
                HeadingEntry {
                    line: block.start_line,
                    level,
                    title: navigate::heading_title(&first),
                    block_id,
                }
            })
            .collect()
    }

    /// 見出しのアンカー名（行, 名前）。**同じ名前には番号を足す**（GitHub と同じ）
    fn heading_slugs(&self) -> Vec<(usize, String)> {
        let entries = self.heading_entries();
        let titles: Vec<String> = entries.iter().map(|entry| entry.title.clone()).collect();
        entries
            .iter()
            .map(|entry| entry.line)
            .zip(navigate::unique_slugs(&titles))
            .collect()
    }

    /// アンカー名から見出しの行を引く。
    ///
    /// **名前が合わなければ、見出しの文字そのものでも探す。** 手で書いた
    /// アンカーは、GitHub の規則と少し違うことが多い
    fn find_anchor(&self, anchor: &str) -> Option<usize> {
        let wanted = anchor.to_lowercase();
        let slugs = self.heading_slugs();
        if let Some((line, _)) = slugs.iter().find(|(_, slug)| *slug == wanted) {
            return Some(*line);
        }
        let wanted_slug = navigate::slug(anchor);
        slugs
            .iter()
            .find(|(_, slug)| *slug == wanted_slug)
            .map(|(line, _)| *line)
    }

    /// コードの外の行を順に渡す（リンクを集めるため）。
    fn for_each_prose_line(&self, mut visit: impl FnMut(usize, &str)) {
        for block in self.document.blocks() {
            if matches!(block.kind, BlockKind::Code { .. }) {
                continue;
            }
            for line in block.start_line..block.start_line + block.line_count {
                visit(line, &self.line_text(line));
            }
        }
    }

    /// 同じ言語のコードブロックの中身（行番号, 行）。
    fn code_lines(&self, language: &str) -> Vec<(usize, String)> {
        let mut lines = Vec::new();
        for block in self.document.blocks() {
            let BlockKind::Code { language: found } = &block.kind else {
                continue;
            };
            if found.as_deref().unwrap_or_default() != language {
                continue;
            }
            // フェンスの行は除く
            let first = block.start_line + 1;
            let last = block.start_line + block.line_count;
            for line in first..last {
                let content = self.line_text(line);
                let trimmed = content.trim_start();
                if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                    continue;
                }
                lines.push((line, content));
            }
        }
        lines
    }

    /// 参照の定義（名前 → 行）と脚注の定義。
    fn definitions(&self) -> (HashMap<String, (usize, String)>, HashMap<String, usize>) {
        let mut references = HashMap::new();
        let mut footnotes = HashMap::new();
        self.for_each_prose_line(|line, content| {
            if let Some((note, label, target)) = navigate::definition_in(content) {
                if note {
                    footnotes.entry(label).or_insert(line);
                } else {
                    references.entry(label).or_insert((line, target));
                }
            }
        });
        (references, footnotes)
    }

    // --- 動かす ---

    /// 行へ飛ぶ。**少し上に余白を残し**、プレビューも寄せる（検索と同じ）
    pub(super) fn go_to(&mut self, line: usize, column: usize) {
        let line = line.min(self.document.text().len_lines().saturating_sub(1));
        self.editor.place_caret(line, column);
        self.editor.scroll_carry = 0.0;
        self.ensure_caret_unfolded();
        self.editor.top_line = self.folds.step(line, -3, self.document.text().len_lines());
        if self.mode != ViewMode::Edit {
            self.preview.anchor = sync::to_preview(&self.document, line);
        }
    }

    /// 結果を見せる。1 件なら飛び、2 件以上なら一覧を出す。
    fn show_results(&mut self, title: String, items: Vec<ResultItem>, empty: &str) {
        match items.len() {
            0 => self.notice = Some(notice::Notice::plain(empty.to_owned())),
            1 => {
                self.results = None;
                self.go_to(items[0].line, items[0].column);
            }
            count => {
                let shown = items.into_iter().take(MAX_RESULTS).collect::<Vec<_>>();
                let suffix = if count > MAX_RESULTS {
                    format!("（先頭の {MAX_RESULTS} 件）")
                } else {
                    String::new()
                };
                self.results = Some(Results {
                    title: format!("{title} {count} 件{suffix}"),
                    items: shown,
                });
            }
        }
    }

    fn item_for(&self, line: usize) -> ResultItem {
        let content = self.line_text(line);
        let column = content.chars().take_while(|ch| ch.is_whitespace()).count();
        ResultItem {
            line,
            column,
            label: format!("{}: {}", line + 1, content.trim()),
        }
    }

    /// 前後の見出しへ（R-18）。
    pub(super) fn heading_step(&mut self, forward: bool) {
        let here = self.editor.cursor_line;
        let lines: Vec<usize> = self
            .document
            .headings()
            .map(|(_, _, block)| block.start_line)
            .filter(|line| !self.folds.is_hidden(*line))
            .collect();
        let target = if forward {
            lines.iter().copied().find(|line| *line > here)
        } else {
            lines.iter().rev().copied().find(|line| *line < here)
        };
        match target {
            Some(line) => {
                self.editor.anchor = None;
                let byte = self.document.byte_at(line, 0);
                self.move_caret_to_byte(byte);
                if self.mode != ViewMode::Edit {
                    self.preview.anchor = sync::to_preview(&self.document, line);
                }
            }
            None => {
                self.notice = Some(notice::Notice::plain(if forward {
                    "この先に見出しはありません".to_owned()
                } else {
                    "この前に見出しはありません".to_owned()
                }))
            }
        }
    }

    /// 閉じ括弧へ（R-07）。
    pub(super) fn closing_bracket(&mut self) {
        let text = self.document.text().to_string();
        match navigate::enclosing_close(&text, self.caret_byte()) {
            Some(at) => {
                self.editor.anchor = None;
                self.move_caret_to_byte(at);
            }
            None => {
                self.notice = Some(notice::Notice::plain(
                    "キャレットを囲む括弧が見つかりません".to_owned(),
                ))
            }
        }
    }

    /// 定義・型定義・宣言・実装・参照へ（R-07）。
    pub(super) fn seek(&mut self, seek: Seek) -> Task<Message> {
        let line = self.editor.cursor_line;
        let content = self.line_text(line);
        let caret = super::features::byte_of_column(&content, self.editor.cursor_column);

        // **コードの中は識別子を字句で探す**（要件定義書 §0.2）
        if let Some(language) = self.code_language_at(line) {
            let Some((name, _)) = navigate::identifier_at(&content, caret) else {
                self.notice = Some(notice::Notice::plain(
                    "キャレットの位置に識別子がありません".to_owned(),
                ));
                return Task::none();
            };
            let lines = self.code_lines(&language);
            let refs: Vec<(usize, &str)> = lines.iter().map(|(n, l)| (*n, l.as_str())).collect();
            let found = navigate::seek_identifier(seek, &name, &refs);
            let items = found.into_iter().map(|line| self.item_for(line)).collect();
            self.show_results(
                format!("{}「{name}」", seek.label()),
                items,
                &format!(
                    "{}「{name}」が見つかりません（同じ言語のコードブロックを書き方の目安で探しています）",
                    seek.label()
                ),
            );
            return Task::none();
        }

        if seek == Seek::References {
            self.markdown_references(line, &content, caret);
            return Task::none();
        }
        self.markdown_definition(line, &content, caret)
    }

    /// 本文での「定義へ」（R-07）。
    fn markdown_definition(&mut self, line: usize, content: &str, caret: usize) -> Task<Message> {
        let Some(link) = navigate::link_at(content, caret) else {
            let message = if navigate::definition_in(content).is_some() {
                "ここが定義です（使っている箇所は「参照を探す」で一覧にできます）"
            } else {
                "キャレットの位置にリンク・参照・脚注がありません"
            };
            self.notice = Some(notice::Notice::plain(message.to_owned()));
            return Task::none();
        };
        let _ = line;
        let (references, footnotes) = self.definitions();
        match link.kind {
            LinkKind::Footnote { label } => match footnotes.get(&label) {
                Some(&at) => self.go_to(at, 0),
                None => {
                    self.notice = Some(notice::Notice::plain(format!(
                        "脚注「^{label}」の定義がありません"
                    )))
                }
            },
            LinkKind::Reference { label } => match references.get(&label) {
                Some(&(at, _)) => self.go_to(at, 0),
                None => {
                    self.notice = Some(notice::Notice::plain(format!(
                        "参照「{label}」の定義がありません"
                    )))
                }
            },
            LinkKind::Inline { target } => return self.open_target(&target),
        }
        Task::none()
    }

    /// 本文での「参照を探す」（R-07）。
    fn markdown_references(&mut self, line: usize, content: &str, caret: usize) {
        // 何を探すか: 見出し（アンカー）・参照の名前・脚注の名前
        enum Wanted {
            Anchor(String),
            Reference(String),
            Footnote(String),
        }
        let heading_line = self
            .document
            .headings()
            .any(|(_, _, block)| block.start_line == line);

        let wanted = if heading_line {
            self.heading_slugs()
                .into_iter()
                .find(|(at, _)| *at == line)
                .map(|(_, slug)| Wanted::Anchor(slug))
        } else if let Some((note, label, _)) = navigate::definition_in(content) {
            Some(if note {
                Wanted::Footnote(label)
            } else {
                Wanted::Reference(label)
            })
        } else {
            navigate::link_at(content, caret).and_then(|link| match link.kind {
                LinkKind::Footnote { label } => Some(Wanted::Footnote(label)),
                LinkKind::Reference { label } => Some(Wanted::Reference(label)),
                LinkKind::Inline { target } => match navigate::classify(&target) {
                    Target::Anchor(anchor) => Some(Wanted::Anchor(anchor.to_lowercase())),
                    _ => None,
                },
            })
        };
        let Some(wanted) = wanted else {
            self.notice = Some(notice::Notice::plain(
                "見出し・参照の定義・脚注・リンクの上で使ってください".to_owned(),
            ));
            return;
        };

        let mut items = Vec::new();
        self.for_each_prose_line(|at, text| {
            for link in navigate::links_in(text) {
                let hit = match (&wanted, &link.kind) {
                    (Wanted::Footnote(want), LinkKind::Footnote { label }) => want == label,
                    (Wanted::Reference(want), LinkKind::Reference { label }) => want == label,
                    (Wanted::Anchor(want), LinkKind::Inline { target }) => {
                        matches!(navigate::classify(target), Target::Anchor(a) if a.to_lowercase() == *want)
                    }
                    _ => false,
                };
                if hit {
                    let column = text[..link.range.start].chars().count();
                    items.push(ResultItem {
                        line: at,
                        column,
                        label: format!("{}: {}", at + 1, text.trim()),
                    });
                }
            }
        });
        let title = match &wanted {
            Wanted::Anchor(slug) => format!("「#{slug}」への参照"),
            Wanted::Reference(label) => format!("「{label}」の参照"),
            Wanted::Footnote(label) => format!("脚注「^{label}」の参照"),
        };
        self.show_results(title, items, "参照は見つかりませんでした");
    }

    /// キャレットのリンクを開く（R-19）。
    pub(super) fn open_link_at(&mut self, line: usize, column: usize) -> Task<Message> {
        let content = self.line_text(line);
        let caret = super::features::byte_of_column(&content, column);
        let Some(link) = navigate::link_at(&content, caret) else {
            self.notice = Some(notice::Notice::plain(
                "キャレットの位置にリンクがありません".to_owned(),
            ));
            return Task::none();
        };
        let (references, footnotes) = self.definitions();
        match link.kind {
            LinkKind::Inline { target } => self.open_target(&target),
            // **参照リンクは定義の先を開く**（押した人が見たいのはリンク先）
            LinkKind::Reference { label } => match references.get(&label) {
                Some((_, target)) if !target.is_empty() => {
                    let target = target.clone();
                    self.open_target(&target)
                }
                Some(&(at, _)) => {
                    self.go_to(at, 0);
                    Task::none()
                }
                None => {
                    self.notice = Some(notice::Notice::plain(format!(
                        "参照「{label}」の定義がありません"
                    )));
                    Task::none()
                }
            },
            LinkKind::Footnote { label } => {
                match footnotes.get(&label) {
                    Some(&at) => self.go_to(at, 0),
                    None => {
                        self.notice = Some(notice::Notice::plain(format!(
                            "脚注「^{label}」の定義がありません"
                        )))
                    }
                }
                Task::none()
            }
        }
    }

    /// リンクの先を開く（R-19）。
    ///
    /// `#見出し` はその見出しへ、Markdown のファイルは**別の窓**で（R-09）、
    /// それ以外は OS の既定のアプリで開く
    pub(super) fn open_target(&mut self, target: &str) -> Task<Message> {
        match navigate::classify(target) {
            Target::Anchor(anchor) => match self.find_anchor(&anchor) {
                Some(line) => self.go_to(line, 0),
                None => {
                    self.notice = Some(notice::Notice::plain(format!(
                        "見出し「#{anchor}」が見つかりません"
                    )))
                }
            },
            Target::External(url) => {
                if let Err(error) = self.launch_external(&url) {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{url} を開けません: {error}"
                    )));
                }
            }
            Target::File { path, anchor } => {
                if path.is_empty() {
                    if let Some(anchor) = anchor {
                        return self.open_target(&format!("#{anchor}"));
                    }
                    return Task::none();
                }
                let candidate = std::path::PathBuf::from(&path);
                let resolved = if candidate.is_absolute() {
                    candidate
                } else {
                    match self.meta.base_dir() {
                        Some(base) => base.join(candidate),
                        None => {
                            self.notice = Some(notice::Notice::plain(
                                "相対パスのリンクは、文書を保存してから開けます".to_owned(),
                            ));
                            return Task::none();
                        }
                    }
                };
                if !resolved.exists() {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} が見つかりません",
                        resolved.display()
                    )));
                    return Task::none();
                }
                // 自分自身へのリンクなら、この窓で見出しへ
                let same = self.meta.path.as_ref().is_some_and(|current| {
                    std::fs::canonicalize(current).ok() == std::fs::canonicalize(&resolved).ok()
                });
                if same {
                    if let Some(anchor) = anchor {
                        return self.open_target(&format!("#{anchor}"));
                    }
                    return Task::none();
                }
                if crate::io::is_markdown(&resolved) {
                    self.spawn_window(Some(&resolved));
                } else if let Err(error) = self.launch_external(&resolved.display().to_string()) {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} を開けません: {error}",
                        resolved.display()
                    )));
                }
            }
        }
        Task::none()
    }

    /// プレビューで押されたリンク（R-19）。
    ///
    /// **描画層は押した文字しか知らない。** ブロックの原文からリンクを拾い直し、
    /// 文字が合うものを開く
    pub(super) fn preview_link(&mut self, block: usize, clicked: &str) -> Task<Message> {
        let Some(found) = self.document.blocks().get(block).cloned() else {
            return Task::none();
        };
        let clicked = clicked.trim();
        let (references, _) = self.definitions();
        let mut candidates: Vec<(String, String)> = Vec::new();
        for line in found.start_line..found.start_line + found.line_count {
            let content = self.line_text(line);
            for link in navigate::links_in(&content) {
                let source = &content[link.range.clone()];
                // 見えている文字（`[` と `]` の間、または URL そのもの）
                let shown = source
                    .trim_start_matches('!')
                    .strip_prefix('[')
                    .and_then(|rest| rest.split(']').next())
                    .unwrap_or(source)
                    .to_owned();
                let target = match &link.kind {
                    LinkKind::Inline { target } => target.clone(),
                    LinkKind::Reference { label } => references
                        .get(label)
                        .map(|(_, target)| target.clone())
                        .unwrap_or_default(),
                    LinkKind::Footnote { .. } => continue,
                };
                if !target.is_empty() {
                    candidates.push((shown, target));
                }
            }
        }
        let chosen = candidates
            .iter()
            .find(|(shown, _)| {
                shown.contains(clicked) || (!shown.is_empty() && clicked.contains(shown.as_str()))
            })
            .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
            .map(|(_, target)| target.clone());
        match chosen {
            Some(target) => self.open_target(&target),
            None => Task::none(),
        }
    }

    /// リンク切れを調べる（R-19）。**`http(s)` は調べない**（ネットワークへ出ない）
    pub(super) fn check_links(&mut self) {
        let slugs: HashSet<String> = self
            .heading_slugs()
            .into_iter()
            .map(|(_, slug)| slug)
            .collect();
        let (references, footnotes) = self.definitions();
        let base = self.meta.base_dir().map(std::path::Path::to_path_buf);
        let mut items = Vec::new();
        let mut unsaved_relative = false;

        self.for_each_prose_line(|line, content| {
            for link in navigate::links_in(content) {
                let problem = match &link.kind {
                    LinkKind::Inline { target } => match navigate::classify(target) {
                        Target::Anchor(anchor) => {
                            let lower = anchor.to_lowercase();
                            (!slugs.contains(&lower) && !slugs.contains(&navigate::slug(&anchor)))
                                .then(|| format!("見出し「#{anchor}」がありません"))
                        }
                        Target::External(_) => None,
                        Target::File { path, .. } if path.is_empty() => None,
                        Target::File { path, .. } => {
                            let candidate = std::path::PathBuf::from(&path);
                            let resolved = if candidate.is_absolute() {
                                Some(candidate)
                            } else {
                                base.as_ref().map(|base| base.join(&candidate))
                            };
                            match resolved {
                                Some(resolved) if !resolved.exists() => {
                                    Some(format!("ファイルが見つかりません: {path}"))
                                }
                                Some(_) => None,
                                None => {
                                    unsaved_relative = true;
                                    None
                                }
                            }
                        }
                    },
                    // **`[a][b]` の形だけを見る。** `[a]` だけのものは、
                    // 定義が無ければただの文字である
                    LinkKind::Reference { label } => (content[link.range.clone()].contains("][")
                        && !references.contains_key(label))
                    .then(|| format!("参照「{label}」の定義がありません")),
                    LinkKind::Footnote { label } => (!footnotes.contains_key(label))
                        .then(|| format!("脚注「^{label}」の定義がありません")),
                };
                if let Some(problem) = problem {
                    let column = content[..link.range.start].chars().count();
                    items.push(ResultItem {
                        line,
                        column,
                        label: format!("{}: {problem}", line + 1),
                    });
                }
            }
        });

        if items.is_empty() {
            self.results = None;
            self.notice = Some(notice::Notice::plain(if unsaved_relative {
                "リンク切れは見つかりませんでした（相対パスは、保存すると調べられます）".to_owned()
            } else {
                "リンク切れは見つかりませんでした".to_owned()
            }));
            return;
        }
        let count = items.len();
        self.results = Some(Results {
            title: format!("リンク切れ {count} 件"),
            items: items.into_iter().take(MAX_RESULTS).collect(),
        });
    }

    /// 一覧から選んだ。
    pub(super) fn pick_result(&mut self, index: usize) {
        let Some(item) = self
            .results
            .as_ref()
            .and_then(|r| r.items.get(index))
            .cloned()
        else {
            return;
        };
        self.go_to(item.line, item.column);
    }

    /// 一覧（画面の下）。
    pub(super) fn results_view<'a>(&self, results: &'a Results) -> Element<'a, Message> {
        let items = column(results.items.iter().enumerate().map(|(index, item)| {
            button(text(&item.label).size(12))
                .padding([2, 6])
                .width(Length::Fill)
                .style(button::text)
                .on_press(Message::ResultPick(index))
                .into()
        }));
        container(
            column![
                row![
                    text(&results.title).size(12),
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("閉じる").size(11))
                        .padding([2, 8])
                        .on_press(Message::CloseResults),
                ]
                .align_y(iced::Alignment::Center),
                scrollable(items).height(Length::Fixed(140.0)),
            ]
            .spacing(4),
        )
        .padding(6)
        .style(container::bordered_box)
        .into()
    }

    /// 見出しの絞り込み（R-18）。
    pub(super) fn heading_picker_view<'a>(
        &self,
        picker: &'a HeadingPicker,
    ) -> Element<'a, Message> {
        let entries = self.heading_entries();
        let shown: Vec<(usize, HeadingEntry)> = entries
            .into_iter()
            .enumerate()
            .filter(|(_, entry)| matches_filter(&entry.title, &picker.query))
            .take(MAX_PICKER)
            .collect();
        let list = column(shown.into_iter().map(|(index, entry)| {
            let indent = f32::from(entry.level.saturating_sub(1)) * 12.0;
            container(
                button(text(entry.title).size(12))
                    .padding([2, 6])
                    .width(Length::Fill)
                    .style(button::text)
                    .on_press(Message::HeadingPickerPick(index)),
            )
            .padding(iced::Padding {
                left: indent,
                ..iced::Padding::ZERO
            })
            .into()
        }));
        container(
            column![
                row![
                    text("見出しへ移動").size(12),
                    text_input("見出しの文字で絞り込む", &picker.query)
                        .id(super::heading_picker_id())
                        .on_input(Message::HeadingPickerInput)
                        .on_submit(Message::HeadingPickerSubmit)
                        .size(13)
                        .width(Length::Fixed(320.0)),
                    button(text("閉じる").size(12))
                        .padding([4, 10])
                        .on_press(Message::CloseHeadingPicker),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                scrollable(list).height(Length::Fixed(220.0)),
            ]
            .spacing(6),
        )
        .padding(6)
        .into()
    }

    /// 絞り込みから選んだ（添字は見出し全体の中の位置）。
    pub(super) fn pick_heading(&mut self, index: usize) {
        let Some(entry) = self.heading_entries().into_iter().nth(index) else {
            return;
        };
        self.heading_picker = None;
        self.go_to(entry.line, 0);
        // 見出しを画面の一番上にする（目次で飛ぶのと同じ）
        self.editor.top_line = entry.line;
        self.preview.anchor = crate::layout::ScrollAnchor {
            block_id: entry.block_id,
            offset_in_block: 0.0,
        };
    }

    /// 絞り込みで `Enter` を押した。**いちばん上のものへ飛ぶ**
    pub(super) fn submit_heading_picker(&mut self) {
        let Some(picker) = &self.heading_picker else {
            return;
        };
        let query = picker.query.clone();
        let first = self
            .heading_entries()
            .iter()
            .position(|entry| matches_filter(&entry.title, &query));
        match first {
            Some(index) => self.pick_heading(index),
            None => {
                self.notice = Some(notice::Notice::plain(format!(
                    "「{query}」を含む見出しがありません"
                )))
            }
        }
    }

    // --- 折りたたみ（R-20） ---

    /// 見出しの一覧と隠す範囲を作り直す。**編集のたびに呼ぶ**
    pub(super) fn rebuild_folds(&mut self) {
        self.fold_headings = self
            .document
            .headings()
            .map(|(_, level, block)| fold::Heading {
                line: block.start_line,
                level,
                revision: block.revision,
            })
            .collect();
        let total = self.document.text().len_lines();
        self.folds = fold::build(&self.fold_headings, &mut self.folded, total);
        // **先頭行を隠れたところに残さない**（描く行が無くなる）
        if self.folds.is_hidden(self.editor.top_line) {
            self.editor.top_line = self.folds.visible_at_or_before(self.editor.top_line);
        }
    }

    /// キャレットが隠れたところへ入ったら、そこを開く（R-20）。
    ///
    /// **畳んだ中で編集させない。** 見えないところが書き換わる
    pub(super) fn ensure_caret_unfolded(&mut self) {
        if self.folded.is_empty() || !self.folds.is_hidden(self.editor.cursor_line) {
            return;
        }
        let total = self.document.text().len_lines();
        let line = self.editor.cursor_line;
        let headings = self.fold_headings.clone();
        let opened: BTreeSet<u64> = headings
            .iter()
            .enumerate()
            .filter(|(index, heading)| {
                self.folded.contains(&heading.revision)
                    && fold::section(&headings, *index, total).contains(&line)
            })
            .map(|(_, heading)| heading.revision)
            .collect();
        self.folded.retain(|revision| !opened.contains(revision));
        self.rebuild_folds();
    }

    /// 畳む。**キャレットは見出しの行へ移す**（隠れたところに置かない）
    pub(super) fn fold_here(&mut self) {
        let Some(index) = fold::heading_for(&self.fold_headings, self.editor.cursor_line) else {
            self.notice = Some(notice::Notice::plain(
                "キャレットより前に見出しがありません".to_owned(),
            ));
            return;
        };
        let heading = self.fold_headings[index];
        self.folded.insert(heading.revision);
        self.rebuild_folds();
        if self.folds.is_hidden(self.editor.cursor_line) {
            self.editor.place_caret(heading.line, 0);
        }
    }

    /// 開く。キャレットのある節の見出しを開く
    pub(super) fn unfold_here(&mut self) {
        let Some(index) = fold::heading_for(&self.fold_headings, self.editor.cursor_line) else {
            return;
        };
        let revision = self.fold_headings[index].revision;
        self.folded.remove(&revision);
        self.rebuild_folds();
    }

    pub(super) fn fold_all(&mut self) {
        self.folded = self.fold_headings.iter().map(|h| h.revision).collect();
        self.rebuild_folds();
        if self.folds.is_hidden(self.editor.cursor_line) {
            let line = self.folds.visible_at_or_before(self.editor.cursor_line);
            self.editor.place_caret(line, 0);
        }
    }

    pub(super) fn unfold_all(&mut self) {
        self.folded.clear();
        self.rebuild_folds();
    }

    /// 行番号の欄の印を押した。
    pub(super) fn toggle_fold_at(&mut self, line: usize) {
        let Some(heading) = self.fold_headings.iter().find(|h| h.line == line).copied() else {
            return;
        };
        if !self.folded.remove(&heading.revision) {
            self.folded.insert(heading.revision);
        }
        self.rebuild_folds();
        if self.folds.is_hidden(self.editor.cursor_line) {
            self.editor.place_caret(heading.line, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file::DocumentMeta;
    use crate::render::{Action, CursorMove};

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn put_caret(app: &mut App, line: usize, column: usize) {
        let _ = app.update(Message::Editor(Action::Move {
            movement: CursorMove::To { line, column },
            select: false,
        }));
    }

    #[test]
    fn heading_steps_move_between_headings() {
        let mut app = app("# 一\n本文\n## 二\n本文\n# 三\n");
        let _ = app.update(Message::HeadingStep(true));
        assert_eq!(app.editor.cursor_line, 2);
        let _ = app.update(Message::HeadingStep(true));
        assert_eq!(app.editor.cursor_line, 4);
        let _ = app.update(Message::HeadingStep(false));
        assert_eq!(app.editor.cursor_line, 2);
    }

    #[test]
    fn definition_jumps_to_a_footnote() {
        let mut app = app("本文[^1]です\n\n[^1]: 脚注\n");
        put_caret(&mut app, 0, 3);
        let _ = app.update(Message::Seek(Seek::Definition));
        assert_eq!(app.editor.cursor_line, 2);
    }

    #[test]
    fn definition_jumps_to_an_anchor() {
        let mut app = app("# 概要\n\n[見る](#概要)\n");
        put_caret(&mut app, 2, 2);
        let _ = app.update(Message::Seek(Seek::Definition));
        assert_eq!(app.editor.cursor_line, 0);
    }

    #[test]
    fn references_to_a_heading_are_listed() {
        let mut app = app("# 概要\n[a](#概要)\n[b](#概要)\n");
        put_caret(&mut app, 0, 0);
        let _ = app.update(Message::Seek(Seek::References));
        let results = app.results.as_ref().expect("一覧が出る");
        assert_eq!(results.items.len(), 2);
    }

    #[test]
    fn code_definitions_are_found_in_the_same_language() {
        let mut app = app("```rust\nfn add() {}\n```\n\n```rust\nlet x = add();\n```\n");
        put_caret(&mut app, 5, 9);
        let _ = app.update(Message::Seek(Seek::Definition));
        assert_eq!(app.editor.cursor_line, 1);
    }

    #[test]
    fn broken_links_are_reported() {
        let mut app = app("# 在る\n[a](#在る)\n[b](#無い)\n[c][未定義]\n[^9]\n");
        let _ = app.update(Message::CheckLinks);
        let results = app.results.as_ref().expect("一覧が出る");
        let lines: Vec<usize> = results.items.iter().map(|item| item.line).collect();
        assert_eq!(lines, [2, 3, 4]);
    }

    #[test]
    fn the_closing_bracket_is_reached() {
        let mut app = app("f(a, [b])");
        put_caret(&mut app, 0, 6);
        let _ = app.update(Message::ClosingBracket);
        assert_eq!(app.editor.cursor_column, 7);
        let _ = app.update(Message::ClosingBracket);
        assert_eq!(app.editor.cursor_column, 8);
    }

    #[test]
    fn folding_hides_a_section_and_opens_on_entry() {
        let mut app = app("# 一\na\nb\n# 二\nc\n");
        put_caret(&mut app, 1, 0);
        let _ = app.update(Message::Fold);
        assert!(app.folds.is_hidden(1) && app.folds.is_hidden(2));
        assert!(!app.folds.is_hidden(3));
        assert_eq!(app.editor.cursor_line, 0, "見出しへ移る");
        // 下へ動くと、隠れた行を飛ばす
        let _ = app.update(Message::Editor(Action::Move {
            movement: CursorMove::Down,
            select: false,
        }));
        assert_eq!(app.editor.cursor_line, 3);
        // 行へジャンプで中へ入ったら開く
        app.go_to(2, 0);
        assert!(!app.folds.is_hidden(2));
    }

    #[test]
    fn the_heading_filter_is_case_insensitive() {
        assert!(matches_filter("Getting Started", "start"));
        assert!(matches_filter("何でも", ""));
        assert!(!matches_filter("概要", "詳細"));
    }
}
