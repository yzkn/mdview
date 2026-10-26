//! フォルダーの比較の画面（v2.6.0 R-04 a）。2 つのフォルダーの下の階層まで、ファイルごとに比べて一覧に出す。
//!
//! **本文を覆う画面**（比較の画面と同じ）。一覧の行を開くと、そのファイルどうしの 2 面の比較
//! （`super::diff`）が上に開き、閉じるとこの一覧に戻る（開いた行は比べ直す）。
//!
//! 一覧はまず裏で作り（片方にしか無いもの・大きさが違うものはその場で決まる）、残りの中身の照合を
//! **少しずつ裏で**行う。照合の間も画面は止めず、進み具合を出す。計算は `crate::diff::folder`。
//!
//! 行を選んで**まとめて写す**（v2.7.0 R-03）。写す前に作る・上書きする・消すものを見せて確かめ、写す先にしか
//! 無いものを消すのは、そこで選んだときだけ（OS のごみ箱へ）。写すのも少しずつ裏で行い、途中で止められる。

use std::path::{Path, PathBuf};

use iced::widget::{button, checkbox, column, container, row, scrollable, text};
use iced::{Element, Length, Task};

use super::App;
use crate::diff::folder::{self, Direction, Entry, Plan, Status};
use crate::render::Message;

/// 1 回に裏で照合するファイルの数
const CHUNK: usize = 32;

/// 1 回に裏で写す・消すファイルの数
const COPY_CHUNK: usize = 16;

/// まとめて写す（v2.7.0 R-03）。確かめている間と、写している間
#[derive(Debug)]
pub struct CopyState {
    pub direction: Direction,
    pub plan: Plan,
    /// 写す先にしか無いものを消すか（確かめの窓で選ぶ。既定は消さない）
    pub delete_extra: bool,
    /// 写し始めたか（`false` なら確かめている）
    pub running: bool,
    /// 残り（行の番号と、消すものか）
    queue: Vec<(usize, bool)>,
    /// 済んだ行
    touched: Vec<usize>,
    /// 裏で写している最中のもの（行の番号・消すものか・相対パス）
    pub in_flight: Vec<(usize, bool, PathBuf)>,
    pub done: usize,
    pub total: usize,
    /// ごみ箱へ入れた数
    pub trashed: usize,
    /// 写せなかった・消せなかったもの
    pub failures: Vec<(PathBuf, String)>,
    /// 止めるよう頼まれた
    pub cancelled: bool,
}

/// フォルダーの比較の画面
#[derive(Debug)]
pub struct FolderScreen {
    pub left: PathBuf,
    pub right: PathBuf,
    pub entries: Vec<Entry>,
    /// 違うものだけを出すか
    pub only_different: bool,
    /// 頼んだ番号（**古い答えは捨てる**。比べ直したとき）
    pub generation: u64,
    /// 一覧を作っている最中か
    pub listing: bool,
    /// 読めなかった理由
    pub error: Option<String>,
    /// 2 面の比較で開いている行
    pub opened: Option<usize>,
    /// 選んだ行（一覧と同じ並び。v2.7.0 R-03）
    pub selected: Vec<bool>,
    /// まとめて写す（確かめ・最中・失敗の一覧）
    pub copy: Option<CopyState>,
}

impl FolderScreen {
    /// 中身を照合し終えていないファイルの数
    pub fn pending(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.status.is_none())
            .count()
    }

    /// 選んだ行の数
    pub fn selected_count(&self) -> usize {
        self.selected.iter().filter(|on| **on).count()
    }

    /// 写している最中か
    pub fn copying(&self) -> bool {
        self.copy.as_ref().is_some_and(|copy| copy.running)
    }

    /// 数（同じ・違う・左だけ・右だけ）
    fn counts(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for entry in &self.entries {
            match entry.status {
                Some(Status::Same) => out[0] += 1,
                Some(Status::Different) | Some(Status::Error(_)) => out[1] += 1,
                Some(Status::LeftOnly) => out[2] += 1,
                Some(Status::RightOnly) => out[3] += 1,
                None => {}
            }
        }
        out
    }
}

/// フォルダーの比較の画面へのメッセージ
#[derive(Debug, Clone)]
pub enum FolderMessage {
    /// 「フォルダーを比べる」
    Open,
    PickedLeft(Option<PathBuf>),
    PickedRight {
        left: PathBuf,
        path: Option<PathBuf>,
    },
    /// 一覧を作り終えた
    Listed {
        generation: u64,
        result: Result<Vec<Entry>, String>,
    },
    /// 中身を照合した（行の番号と結果）
    Compared {
        generation: u64,
        results: Vec<(usize, Status)>,
    },
    OnlyDifferent(bool),
    /// 行を開く（2 面の比較）
    OpenRow(usize),
    /// 比べ直す
    Refresh,
    Close,
    /// 行を選ぶ・外す（v2.7.0 R-03）
    Select(usize, bool),
    /// 全部選ぶ・違うものだけ選ぶ・選ぶのをやめる
    SelectAll,
    SelectDifferent,
    SelectNone,
    /// まとめて写す（向き）。確かめる設定なら確かめの窓を出す
    Copy(Direction),
    /// 写す先にしか無いものを消すか
    CopyDeleteExtra(bool),
    /// 確かめの窓の「写す」
    CopyStart,
    /// 確かめの窓の「やめる」・写している最中の「止める」・失敗の一覧の「閉じる」
    CopyCancel,
    /// 写した・消した（行の番号・消したか・結果）
    Copied {
        generation: u64,
        results: Vec<(usize, bool, Result<(), String>)>,
    },
}

impl App {
    /// フォルダーの比較の画面へのメッセージを処理する
    pub(super) fn folder_message(&mut self, message: FolderMessage) -> Task<Message> {
        match message {
            FolderMessage::Open => {
                if self.picking || self.folder_screen.is_some() || self.diff_screen.is_some() {
                    return Task::none();
                }
                self.notice = Some(super::notice::Notice::plain(
                    "比べる 1 つ目（左）のフォルダーを選んでください".to_owned(),
                ));
                return self
                    .ask(super::picker::Picker::folder())
                    .map(|path| Message::Folder(FolderMessage::PickedLeft(path)));
            }
            FolderMessage::PickedLeft(path) => {
                self.picking = false;
                let Some(left) = path else {
                    self.notice = None;
                    return Task::none();
                };
                self.notice = Some(super::notice::Notice::plain(
                    "比べる 2 つ目（右）のフォルダーを選んでください".to_owned(),
                ));
                return self
                    .ask(
                        super::picker::Picker::folder()
                            .in_directory(left.parent().map(Path::to_path_buf)),
                    )
                    .map(move |path| {
                        Message::Folder(FolderMessage::PickedRight {
                            left: left.clone(),
                            path,
                        })
                    });
            }
            FolderMessage::PickedRight { left, path } => {
                self.picking = false;
                let Some(right) = path else {
                    self.notice = None;
                    return Task::none();
                };
                self.notice = None;
                self.folder_screen = Some(FolderScreen {
                    left,
                    right,
                    entries: Vec::new(),
                    only_different: false,
                    generation: 0,
                    listing: true,
                    error: None,
                    opened: None,
                    selected: Vec::new(),
                    copy: None,
                });
                return self.folder_list();
            }
            FolderMessage::Listed { generation, result } => {
                let Some(screen) = self.folder_screen.as_mut() else {
                    return Task::none();
                };
                if screen.generation != generation {
                    return Task::none();
                }
                screen.listing = false;
                match result {
                    Ok(entries) => {
                        screen.selected = vec![false; entries.len()];
                        screen.entries = entries;
                        screen.error = None;
                    }
                    Err(error) => screen.error = Some(error),
                }
                return self.folder_compare_next();
            }
            FolderMessage::Compared {
                generation,
                results,
            } => {
                let Some(screen) = self.folder_screen.as_mut() else {
                    return Task::none();
                };
                if screen.generation != generation {
                    return Task::none();
                }
                for (index, status) in results {
                    if let Some(entry) = screen.entries.get_mut(index) {
                        entry.status = Some(status);
                    }
                }
                return self.folder_compare_next();
            }
            FolderMessage::OnlyDifferent(on) => {
                if let Some(screen) = self.folder_screen.as_mut() {
                    screen.only_different = on;
                }
            }
            FolderMessage::OpenRow(index) => return self.folder_open_row(index),
            FolderMessage::Refresh => {
                if let Some(screen) = self.folder_screen.as_mut() {
                    if screen.copying() {
                        return Task::none();
                    }
                    screen.listing = true;
                    screen.entries.clear();
                    screen.selected.clear();
                    screen.copy = None;
                }
                return self.folder_list();
            }
            FolderMessage::Close => {
                if self
                    .folder_screen
                    .as_ref()
                    .is_some_and(FolderScreen::copying)
                {
                    // 写している最中は閉じない（止めてから）
                    return Task::none();
                }
                self.folder_screen = None;
                self.notice = None;
            }
            FolderMessage::Select(index, on) => {
                if let Some(slot) = self
                    .folder_screen
                    .as_mut()
                    .and_then(|screen| screen.selected.get_mut(index))
                {
                    *slot = on;
                }
            }
            FolderMessage::SelectAll
            | FolderMessage::SelectDifferent
            | FolderMessage::SelectNone => {
                if let Some(screen) = self.folder_screen.as_mut() {
                    for (slot, entry) in screen.selected.iter_mut().zip(&screen.entries) {
                        *slot = match message {
                            FolderMessage::SelectAll => true,
                            FolderMessage::SelectDifferent => entry.status != Some(Status::Same),
                            _ => false,
                        };
                    }
                }
            }
            FolderMessage::Copy(direction) => return self.folder_copy(direction),
            FolderMessage::CopyDeleteExtra(on) => {
                if let Some(copy) = self.folder_screen.as_mut().and_then(|s| s.copy.as_mut()) {
                    if !copy.running {
                        copy.delete_extra = on;
                    }
                }
            }
            FolderMessage::CopyStart => return self.folder_copy_start(),
            FolderMessage::CopyCancel => {
                let Some(screen) = self.folder_screen.as_mut() else {
                    return Task::none();
                };
                match screen.copy.as_mut() {
                    Some(copy) if copy.running => copy.cancelled = true,
                    _ => screen.copy = None,
                }
            }
            FolderMessage::Copied {
                generation,
                results,
            } => return self.folder_copied(generation, results),
        }
        Task::none()
    }

    /// 一覧を裏で作る
    fn folder_list(&mut self) -> Task<Message> {
        let ignore = folder::parse_ignore(&self.settings.diff_folder_ignore);
        let Some(screen) = self.folder_screen.as_mut() else {
            return Task::none();
        };
        screen.generation += 1;
        let generation = screen.generation;
        let (left, right) = (screen.left.clone(), screen.right.clone());
        Task::perform(
            async move { folder::list(&left, &right, &ignore) },
            move |result| Message::Folder(FolderMessage::Listed { generation, result }),
        )
    }

    /// まだ照合していないファイルを、少しだけ裏で照合する（終わったら次を頼む）
    fn folder_compare_next(&mut self) -> Task<Message> {
        let Some(screen) = self.folder_screen.as_ref() else {
            return Task::none();
        };
        let work: Vec<(usize, PathBuf, PathBuf)> = screen
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.status.is_none())
            .take(CHUNK)
            .map(|(index, entry)| {
                (
                    index,
                    screen.left.join(&entry.path),
                    screen.right.join(&entry.path),
                )
            })
            .collect();
        if work.is_empty() {
            return Task::none();
        }
        let generation = screen.generation;
        Task::perform(
            async move {
                work.into_iter()
                    .map(|(index, left, right)| (index, folder::compare(&left, &right)))
                    .collect::<Vec<_>>()
            },
            move |results| {
                Message::Folder(FolderMessage::Compared {
                    generation,
                    results,
                })
            },
        )
    }

    /// 行を 2 面の比較で開く。**片方にしか無いものは、無い側を空の文書で開く**（写して保存すると作れる）
    fn folder_open_row(&mut self, index: usize) -> Task<Message> {
        if self.diff_screen.is_some() {
            return Task::none();
        }
        let Some(screen) = self.folder_screen.as_mut() else {
            return Task::none();
        };
        let Some(entry) = screen.entries.get(index) else {
            return Task::none();
        };
        screen.opened = Some(index);
        let left = screen.left.join(&entry.path);
        let right = screen.right.join(&entry.path);
        Task::perform(
            async move {
                let load = |path: &Path| -> Result<Option<Box<crate::io::LoadedFile>>, String> {
                    if !path.exists() {
                        return Ok(None);
                    }
                    crate::io::load_as(path, None)
                        .map(|file| Some(Box::new(file)))
                        .map_err(|error| format!("{}: {error}", path.display()))
                };
                let (a, b) = (load(&left)?, load(&right)?);
                let empty = |path: PathBuf, like: &Option<Box<crate::io::LoadedFile>>| {
                    Box::new(crate::io::LoadedFile {
                        path,
                        text: String::new(),
                        format: like
                            .as_ref()
                            .map(|file| file.format.clone())
                            .unwrap_or_default(),
                        oversized: false,
                        lossy: false,
                    })
                };
                let left_file = a.clone().unwrap_or_else(|| empty(left.clone(), &b));
                let right_file = b.clone().unwrap_or_else(|| empty(right.clone(), &a));
                Ok((left_file, right_file))
            },
            |result| Message::Diff(super::diff::DiffMessage::LoadedFiles(result)),
        )
    }

    /// 2 面の比較を閉じたら、開いていた行を比べ直す（保存したかもしれない）
    pub(super) fn folder_reopen(&mut self) {
        let Some(screen) = self.folder_screen.as_mut() else {
            return;
        };
        let Some(index) = screen.opened.take() else {
            return;
        };
        let Some(entry) = screen.entries.get_mut(index) else {
            return;
        };
        let left = screen.left.join(&entry.path);
        let right = screen.right.join(&entry.path);
        let size = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.len());
        entry.left_size = size(&left);
        entry.right_size = size(&right);
        entry.status = Some(match (entry.left_size, entry.right_size) {
            (Some(_), None) => Status::LeftOnly,
            (None, Some(_)) => Status::RightOnly,
            (Some(a), Some(b)) if a != b => Status::Different,
            (Some(_), Some(_)) => folder::compare(&left, &right),
            (None, None) => Status::Error("どちらにもありません".to_owned()),
        });
    }

    /// まとめて写す（v2.7.0 R-03）。選んだ行から中身を決め、確かめる設定なら確かめの窓を出す
    fn folder_copy(&mut self, direction: Direction) -> Task<Message> {
        let confirm = self.settings.diff_copy_confirm;
        let Some(screen) = self.folder_screen.as_mut() else {
            return Task::none();
        };
        if screen.listing || screen.copy.is_some() {
            return Task::none();
        }
        let plan = folder::plan(&screen.entries, &screen.selected, direction);
        if plan.create.is_empty()
            && plan.overwrite.is_empty()
            && (plan.extra.is_empty() || !confirm)
        {
            self.notice = Some(super::notice::Notice::plain(format!(
                "{}へ写すものがありません（選んだ行が同じか、写す先にしか無いものだけです）",
                direction.label()
            )));
            return Task::none();
        }
        screen.copy = Some(CopyState {
            direction,
            plan,
            delete_extra: false,
            running: false,
            queue: Vec::new(),
            touched: Vec::new(),
            in_flight: Vec::new(),
            done: 0,
            total: 0,
            trashed: 0,
            failures: Vec::new(),
            cancelled: false,
        });
        if confirm {
            return Task::none();
        }
        self.folder_copy_start()
    }

    /// 写し始める
    fn folder_copy_start(&mut self) -> Task<Message> {
        let Some(copy) = self.folder_screen.as_mut().and_then(|s| s.copy.as_mut()) else {
            return Task::none();
        };
        if copy.running {
            return Task::none();
        }
        let mut queue: Vec<(usize, bool)> = copy
            .plan
            .create
            .iter()
            .chain(&copy.plan.overwrite)
            .map(|index| (*index, false))
            .collect();
        if copy.delete_extra {
            queue.extend(copy.plan.extra.iter().map(|index| (*index, true)));
        }
        if queue.is_empty() {
            if let Some(screen) = self.folder_screen.as_mut() {
                screen.copy = None;
            }
            return Task::none();
        }
        // 前から順に取り出すので、逆に並べておく
        queue.reverse();
        copy.total = queue.len();
        copy.queue = queue;
        copy.running = true;
        self.folder_copy_next()
    }

    /// 残りを少しだけ裏で写す・消す
    fn folder_copy_next(&mut self) -> Task<Message> {
        let Some(screen) = self.folder_screen.as_mut() else {
            return Task::none();
        };
        let generation = screen.generation;
        let (from_root, to_root) = {
            let Some(copy) = screen.copy.as_ref() else {
                return Task::none();
            };
            let (from, to) = copy.direction.roots(&screen.left, &screen.right);
            (from.to_path_buf(), to.to_path_buf())
        };
        let Some(copy) = screen.copy.as_mut() else {
            return Task::none();
        };
        let take = copy.queue.len().min(COPY_CHUNK);
        let work: Vec<(usize, bool, PathBuf)> = copy
            .queue
            .split_off(copy.queue.len() - take)
            .into_iter()
            .rev()
            .filter_map(|(index, delete)| {
                screen
                    .entries
                    .get(index)
                    .map(|entry| (index, delete, entry.path.clone()))
            })
            .collect();
        copy.in_flight = work.clone();
        Task::perform(
            async move {
                work.into_iter()
                    .map(|(index, delete, path)| {
                        let result = if delete {
                            crate::io::trash::to_trash(&to_root.join(&path))
                        } else {
                            folder::copy_file(&from_root.join(&path), &to_root.join(&path))
                        };
                        (index, delete, result)
                    })
                    .collect::<Vec<_>>()
            },
            move |results| {
                Message::Folder(FolderMessage::Copied {
                    generation,
                    results,
                })
            },
        )
    }

    /// 写した・消した結果を受け取る。残りがあれば続け、終わったら写した行を比べ直す
    fn folder_copied(
        &mut self,
        generation: u64,
        results: Vec<(usize, bool, Result<(), String>)>,
    ) -> Task<Message> {
        let Some(screen) = self.folder_screen.as_mut() else {
            return Task::none();
        };
        if screen.generation != generation {
            return Task::none();
        }
        let Some(copy) = screen.copy.as_mut() else {
            return Task::none();
        };
        copy.in_flight.clear();
        for (index, delete, result) in results {
            copy.done += 1;
            copy.touched.push(index);
            if delete && result.is_ok() {
                copy.trashed += 1;
            }
            if let Err(error) = result {
                let path = screen
                    .entries
                    .get(index)
                    .map(|entry| entry.path.clone())
                    .unwrap_or_default();
                copy.failures.push((path, error));
            }
        }
        if !copy.cancelled && !copy.queue.is_empty() {
            return self.folder_copy_next();
        }
        // 終わった（か止めた）
        copy.running = false;
        let touched = std::mem::take(&mut copy.touched);
        let (done, failures, trashed, cancelled, label) = (
            copy.done,
            copy.failures.len(),
            copy.trashed,
            copy.cancelled,
            copy.direction.label(),
        );
        if failures == 0 {
            screen.copy = None;
        }
        // 選んだ印は全部外す（同じで写さなかった行も）
        screen.selected.iter_mut().for_each(|on| *on = false);
        // 照合の途中の答えは捨てて、比べ直しを頼み直す（行が減ると番号がずれる）
        screen.generation += 1;
        self.folder_recheck(&touched);
        let mut message = format!("{label}へ {} 件を写しました", done - failures - trashed);
        if trashed > 0 {
            message.push_str(&format!("。{trashed} 件をごみ箱へ入れました"));
        }
        if cancelled {
            message.push_str("（途中で止めました）");
        }
        if failures > 0 {
            message.push_str(&format!("。{failures} 件はできませんでした"));
        }
        self.notice = Some(super::notice::Notice::plain(message));
        self.folder_compare_next()
    }

    /// 写した行の大きさと様子を読み直す（両方に無くなった行は一覧から外す）。中身の照合は裏へ回す
    fn folder_recheck(&mut self, indices: &[usize]) {
        let Some(screen) = self.folder_screen.as_mut() else {
            return;
        };
        let size = |path: &Path| std::fs::metadata(path).ok().map(|meta| meta.len());
        let mut gone = Vec::new();
        for &index in indices {
            let Some(entry) = screen.entries.get_mut(index) else {
                continue;
            };
            entry.left_size = size(&screen.left.join(&entry.path));
            entry.right_size = size(&screen.right.join(&entry.path));
            entry.status = match (entry.left_size, entry.right_size) {
                (Some(_), None) => Some(Status::LeftOnly),
                (None, Some(_)) => Some(Status::RightOnly),
                (Some(a), Some(b)) if a != b => Some(Status::Different),
                (Some(_), Some(_)) => None,
                (None, None) => {
                    gone.push(index);
                    None
                }
            };
            if let Some(slot) = screen.selected.get_mut(index) {
                *slot = false;
            }
        }
        gone.sort_unstable();
        for index in gone.into_iter().rev() {
            screen.entries.remove(index);
            if index < screen.selected.len() {
                screen.selected.remove(index);
            }
        }
    }

    /// まとめて写すの確かめの窓・進み具合・失敗の一覧
    fn folder_copy_panel<'a>(
        &'a self,
        screen: &'a FolderScreen,
        copy: &'a CopyState,
    ) -> Element<'a, Message> {
        let (_, to_root) = copy.direction.roots(&screen.left, &screen.right);
        let name = |index: &usize| {
            screen
                .entries
                .get(*index)
                .map(|entry| entry.path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        };
        let mut body = column![].spacing(4);
        if copy.running {
            body = body.push(
                row![
                    text(format!(
                        "{}へ写しています… {} / {}",
                        copy.direction.label(),
                        copy.done,
                        copy.total
                    ))
                    .size(13)
                    .width(Length::Fill),
                    button(text("止める").size(12))
                        .padding([4, 12])
                        .on_press_maybe(
                            (!copy.cancelled).then_some(Message::Folder(FolderMessage::CopyCancel))
                        ),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            );
        } else if !copy.failures.is_empty() {
            body = body.push(
                row![
                    text(format!("できなかったもの {} 件", copy.failures.len()))
                        .size(13)
                        .width(Length::Fill),
                    button(text("閉じる").size(12))
                        .padding([4, 12])
                        .on_press(Message::Folder(FolderMessage::CopyCancel)),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            );
            let mut list = column![].spacing(1);
            for (path, error) in &copy.failures {
                list = list.push(
                    text(format!(
                        "{}：{error}",
                        path.to_string_lossy().replace('\\', "/")
                    ))
                    .size(11),
                );
            }
            body = body.push(scrollable(list).height(Length::Fixed(120.0)));
        } else {
            let plan = &copy.plan;
            body = body.push(
                text(format!(
                    "{}へ写します（写す先: {}）。作る {}・上書き {}",
                    copy.direction.label(),
                    to_root.display(),
                    plan.create.len(),
                    plan.overwrite.len()
                ))
                .size(13),
            );
            if !plan.extra.is_empty() {
                body = body.push(
                    checkbox(copy.delete_extra)
                        .label(format!(
                            "写す先にしか無い {} 件を消す（OS のごみ箱へ）",
                            plan.extra.len()
                        ))
                        .text_size(12)
                        .on_toggle(|on| Message::Folder(FolderMessage::CopyDeleteExtra(on))),
                );
            }
            let mut list = column![].spacing(1);
            let lines = plan
                .create
                .iter()
                .map(|index| ("作る", index))
                .chain(plan.overwrite.iter().map(|index| ("上書き", index)))
                .chain(
                    plan.extra
                        .iter()
                        .filter(|_| copy.delete_extra)
                        .map(|index| ("消す", index)),
                );
            for (kind, index) in lines.take(500) {
                list = list.push(text(format!("{kind}　{}", name(index))).size(11));
            }
            body = body.push(scrollable(list).height(Length::Fixed(140.0)));
            body = body.push(
                row![
                    button(text("写す").size(12))
                        .padding([4, 14])
                        .on_press(Message::Folder(FolderMessage::CopyStart)),
                    button(text("やめる").size(12))
                        .padding([4, 12])
                        .on_press(Message::Folder(FolderMessage::CopyCancel)),
                ]
                .spacing(8),
            );
        }
        container(body)
            .width(Length::Fill)
            .padding(8)
            .style(container::bordered_box)
            .into()
    }

    /// フォルダーの比較の画面
    pub(super) fn folder_view<'a>(&'a self, screen: &'a FolderScreen) -> Element<'a, Message> {
        let [same, different, left_only, right_only] = screen.counts();
        let pending = screen.pending();
        let summary = if screen.listing {
            "一覧を作っています…".to_owned()
        } else if let Some(error) = &screen.error {
            error.clone()
        } else {
            let mut out = format!(
                "ファイル {}：同じ {same}・違う {different}・左だけ {left_only}・右だけ {right_only}",
                screen.entries.len()
            );
            if pending > 0 {
                out.push_str(&format!("（照合中 あと {pending}）"));
            }
            out
        };
        let header = row![
            text("フォルダーを比べる").size(15),
            text(summary).size(12).width(Length::Fill),
            checkbox(screen.only_different)
                .label("違うものだけ")
                .text_size(12)
                .on_toggle(|on| Message::Folder(FolderMessage::OnlyDifferent(on))),
            button(text("比べ直す").size(12))
                .padding([4, 10])
                .on_press_maybe(
                    (!screen.listing && !screen.copying())
                        .then_some(Message::Folder(FolderMessage::Refresh))
                ),
            button(text("閉じる").size(12))
                .padding([4, 12])
                .on_press_maybe(
                    (!screen.copying()).then_some(Message::Folder(FolderMessage::Close))
                ),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        // 選ぶ・まとめて写す（v2.7.0 R-03）
        let idle = !screen.listing && screen.copy.is_none();
        let small = |label: &'static str, message: FolderMessage, on: bool| {
            button(text(label).size(12))
                .padding([3, 10])
                .on_press_maybe(on.then_some(Message::Folder(message)))
        };
        let selected = screen.selected_count();
        let actions = row![
            small("全部選ぶ", FolderMessage::SelectAll, idle),
            small("違うものだけ選ぶ", FolderMessage::SelectDifferent, idle),
            small(
                "選ぶのをやめる",
                FolderMessage::SelectNone,
                idle && selected > 0
            ),
            text(format!("{selected} 件を選んでいます"))
                .size(12)
                .width(Length::Fill),
            small(
                "左 → 右へ写す…",
                FolderMessage::Copy(Direction::LeftToRight),
                idle && selected > 0
            ),
            small(
                "右 → 左へ写す…",
                FolderMessage::Copy(Direction::RightToLeft),
                idle && selected > 0
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let paths = row![
            text(format!("左: {}", screen.left.display()))
                .size(12)
                .width(Length::FillPortion(1)),
            text(format!("右: {}", screen.right.display()))
                .size(12)
                .width(Length::FillPortion(1)),
        ]
        .spacing(10);
        let size =
            |bytes: Option<u64>| bytes.map_or("—".to_owned(), |bytes| format!("{bytes} B"));
        let mut list = column![].spacing(1);
        for (index, entry) in screen.entries.iter().enumerate() {
            if screen.only_different && entry.status == Some(Status::Same) {
                continue;
            }
            let mark = match &entry.status {
                None => "… 照合中".to_owned(),
                Some(Status::Same) => "＝ 同じ".to_owned(),
                Some(Status::Different) => "≠ 違う".to_owned(),
                Some(Status::LeftOnly) => "← 左だけ".to_owned(),
                Some(Status::RightOnly) => "→ 右だけ".to_owned(),
                Some(Status::Error(reason)) => format!("！ 読めない（{reason}）"),
            };
            let label = row![
                text(mark).size(12).width(Length::Fixed(110.0)),
                text(entry.path.to_string_lossy().replace('\\', "/"))
                    .size(12)
                    .width(Length::Fill),
                text(size(entry.left_size))
                    .size(11)
                    .width(Length::Fixed(100.0)),
                text(size(entry.right_size))
                    .size(11)
                    .width(Length::Fixed(100.0)),
            ]
            .spacing(8);
            let chosen = screen.selected.get(index).copied().unwrap_or(false);
            list = list.push(
                row![
                    checkbox(chosen).on_toggle_maybe(
                        idle.then_some(move |on| Message::Folder(FolderMessage::Select(index, on)))
                    ),
                    button(label)
                        .width(Length::Fill)
                        .padding([2, 6])
                        .style(button::text)
                        .on_press(Message::Folder(FolderMessage::OpenRow(index))),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            );
        }
        container(
            column![
                header,
                paths,
                actions,
                match &screen.copy {
                    Some(copy) => self.folder_copy_panel(screen, copy),
                    None => text(
                        "行を押すと、そのファイルどうしを比べます（片方にしか無いものは、無い側を空で開きます）。印を付けた行はまとめて写せます"
                    )
                    .size(11)
                    .into(),
                },
                container(scrollable(list).height(Length::Fill))
                    .padding(2)
                    .style(container::bordered_box),
            ]
            .spacing(6)
            .padding(8),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentMeta;
    use super::*;

    fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mdview-fc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, body) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn app() -> App {
        let mut app = App::new().0;
        app.replace_document(String::new(), DocumentMeta::untitled());
        app
    }

    /// 選ぶ → 一覧 → 少しずつ照合、の順で一覧ができる（裏の仕事は試験ではその場で流す）
    #[test]
    fn folders_are_listed_then_compared() {
        let left = folder("l", &[("a.md", "同じ"), ("b.md", "左左"), ("only.md", "x")]);
        let right = folder("r", &[("a.md", "同じ"), ("b.md", "右右")]);
        let mut app = app();
        let _ = app.update(Message::Folder(FolderMessage::PickedRight {
            left: left.clone(),
            path: Some(right.clone()),
        }));
        let entries = folder::list(&left, &right, &[]).unwrap();
        let _ = app.update(Message::Folder(FolderMessage::Listed {
            generation: 1,
            result: Ok(entries),
        }));
        let screen = app.folder_screen.as_ref().expect("画面");
        assert_eq!(screen.entries.len(), 3);
        assert_eq!(screen.pending(), 2, "大きさが同じ 2 つは照合を待つ");
        let results = vec![
            (0, folder::compare(&left.join("a.md"), &right.join("a.md"))),
            (1, folder::compare(&left.join("b.md"), &right.join("b.md"))),
        ];
        let _ = app.update(Message::Folder(FolderMessage::Compared {
            generation: 1,
            results,
        }));
        let screen = app.folder_screen.as_ref().unwrap();
        assert_eq!(screen.counts(), [1, 1, 1, 0]);
        // 古い答えは捨てる
        let _ = app.update(Message::Folder(FolderMessage::Compared {
            generation: 0,
            results: vec![(0, Status::Different)],
        }));
        assert_eq!(app.folder_screen.as_ref().unwrap().counts(), [1, 1, 1, 0]);
        let _ = app.update(Message::Folder(FolderMessage::Close));
        assert!(app.folder_screen.is_none());
        let _ = std::fs::remove_dir_all(left);
        let _ = std::fs::remove_dir_all(right);
    }

    /// 裏の仕事（写す）を試験ではその場で流す
    fn run_copy(app: &mut App) {
        for _ in 0..100 {
            let Some(screen) = app.folder_screen.as_ref() else {
                return;
            };
            let Some(copy) = screen.copy.as_ref().filter(|copy| copy.running) else {
                return;
            };
            let (from, to) = copy.direction.roots(&screen.left, &screen.right);
            let results: Vec<_> = copy
                .in_flight
                .iter()
                .map(|(index, delete, path)| {
                    // 試験ではごみ箱へ入れず、その場で消す
                    let result = if *delete {
                        std::fs::remove_file(to.join(path)).map_err(|e| e.to_string())
                    } else {
                        folder::copy_file(&from.join(path), &to.join(path))
                    };
                    (*index, *delete, result)
                })
                .collect();
            let generation = screen.generation;
            let _ = app.update(Message::Folder(FolderMessage::Copied {
                generation,
                results,
            }));
        }
    }

    /// 選んだ行を写し、写す先にしか無いものは選んだときだけ消す。写した行は比べ直す（v2.7.0 R-03）
    #[test]
    fn selected_rows_are_copied_after_confirming() {
        let left = folder(
            "cl",
            &[
                ("same.md", "同じ"),
                ("diff.md", "左の中身"),
                ("sub/new.md", "新しい"),
            ],
        );
        let right = folder(
            "cr",
            &[
                ("same.md", "同じ"),
                ("diff.md", "右"),
                ("extra.md", "右だけ"),
            ],
        );
        let mut app = app();
        app.folder_screen = Some(FolderScreen {
            left: left.clone(),
            right: right.clone(),
            entries: folder::list(&left, &right, &[]).unwrap(),
            only_different: false,
            generation: 1,
            listing: false,
            error: None,
            opened: None,
            selected: Vec::new(),
            copy: None,
        });
        let count = app.folder_screen.as_ref().unwrap().entries.len();
        app.folder_screen.as_mut().unwrap().selected = vec![false; count];
        let _ = app.update(Message::Folder(FolderMessage::SelectAll));
        assert_eq!(app.folder_screen.as_ref().unwrap().selected_count(), 4);
        let _ = app.update(Message::Folder(FolderMessage::Copy(Direction::LeftToRight)));
        let copy = app
            .folder_screen
            .as_ref()
            .unwrap()
            .copy
            .as_ref()
            .expect("確かめる");
        assert!(!copy.running);
        assert_eq!(
            (
                copy.plan.create.len(),
                copy.plan.overwrite.len(),
                copy.plan.extra.len()
            ),
            (1, 2, 1),
            "same.md は照合前なので上書きの側"
        );
        let _ = app.update(Message::Folder(FolderMessage::CopyDeleteExtra(true)));
        let _ = app.update(Message::Folder(FolderMessage::CopyStart));
        run_copy(&mut app);
        assert_eq!(
            std::fs::read_to_string(right.join("diff.md")).unwrap(),
            "左の中身"
        );
        assert_eq!(
            std::fs::read_to_string(right.join("sub/new.md")).unwrap(),
            "新しい"
        );
        assert!(!right.join("extra.md").exists());
        let screen = app.folder_screen.as_ref().unwrap();
        assert!(screen.copy.is_none(), "失敗が無ければ閉じる");
        assert_eq!(screen.entries.len(), 3, "両方から無くなった行は外す");
        assert_eq!(screen.selected_count(), 0);
        assert_eq!(screen.pending(), 3, "写した行は比べ直す");
        // 確かめる設定を切ると、すぐ写す（消すことはしない）
        app.settings.diff_copy_confirm = false;
        std::fs::write(left.join("diff.md"), "また変えた").unwrap();
        let _ = app.update(Message::Folder(FolderMessage::Select(0, true)));
        let _ = app.update(Message::Folder(FolderMessage::Copy(Direction::LeftToRight)));
        assert!(app.folder_screen.as_ref().unwrap().copying());
        run_copy(&mut app);
        assert_eq!(
            std::fs::read_to_string(right.join("diff.md")).unwrap(),
            "また変えた"
        );
        let _ = std::fs::remove_dir_all(left);
        let _ = std::fs::remove_dir_all(right);
    }

    /// 2 面の比較を閉じたら、開いていた行を比べ直す
    #[test]
    fn closing_the_file_diff_rechecks_the_row() {
        let left = folder("l2", &[("a.md", "左")]);
        let right = folder("r2", &[("a.md", "右")]);
        let mut app = app();
        app.folder_screen = Some(FolderScreen {
            left: left.clone(),
            right: right.clone(),
            entries: folder::list(&left, &right, &[]).unwrap(),
            only_different: false,
            generation: 1,
            listing: false,
            error: None,
            opened: Some(0),
            selected: vec![false],
            copy: None,
        });
        std::fs::write(right.join("a.md"), "左").unwrap();
        app.folder_reopen();
        assert_eq!(
            app.folder_screen.as_ref().unwrap().entries[0].status,
            Some(Status::Same)
        );
        let _ = std::fs::remove_dir_all(left);
        let _ = std::fs::remove_dir_all(right);
    }
}
