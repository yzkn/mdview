//! メニューの中身（§7.2）。
//!
//! **並びと有効・無効だけをここで決める。** 画面の組み立ては持たないので、
//! 窓無しで試験できる。
//!
//! **自前で持つ理由**（§7.2）: Windows / Linux では OS のメニューを使わず
//! 自前で描く。アプリが直接キーを受けるため、v1 で起きた
//! 「Windows でアクセラレータが発火しない」が構造的に起きない。

use super::keymap::{Command, Keymap};
use crate::app::Format;
use crate::render::{FileCommand, Message, ViewMode};

/// 見出し（メニューバーに並ぶもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    View,
    /// 移動（v2.1.0 R-07 / R-18 / R-19 / R-20）
    Go,
    Help,
}

impl Menu {
    pub const ALL: [Menu; 5] = [Menu::File, Menu::Edit, Menu::View, Menu::Go, Menu::Help];

    /// `Alt` と合わせて押す文字（§7.2）。
    ///
    /// **Windows の作法に合わせる。** 画面には `ファイル(F)` のように出し、
    /// 割り当て文字へ下線を引く
    pub fn access(self) -> char {
        match self {
            Menu::File => 'F',
            Menu::Edit => 'E',
            Menu::View => 'V',
            Menu::Go => 'G',
            Menu::Help => 'H',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Menu::File => "ファイル",
            Menu::Edit => "編集",
            Menu::View => "表示",
            Menu::Go => "移動",
            Menu::Help => "ヘルプ",
        }
    }
}

/// ファイルメニューの中で折りたためる並び（§19.6）。
///
/// **押すまで開かない。** 文字コードは 13 項目あり、いつも開いていると
/// メニューが窓の高さを超える
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submenu {
    /// 指定して開き直す
    ReopenAs,
    /// この文字コードで保存
    SaveWithEncoding,
    /// 選んだ範囲の変換
    Transform,
    /// 日付・時刻の差し込み
    Insert,
    /// 最近開いたファイル
    Recent,
    /// タブ幅
    TabWidth,
    /// 改行コードの指定
    LineEnding,
    /// 強調・コード・リンク（v2.1.0 R-15）
    Format,
    /// 表（v2.1.0 R-16）
    Table,
}

impl Submenu {
    pub fn label(self) -> &'static str {
        match self {
            Submenu::ReopenAs => "文字コードを指定して開き直す",
            Submenu::SaveWithEncoding => "文字コードを指定して保存",
            Submenu::Transform => "選んだ範囲を変換",
            Submenu::Insert => "日付・時刻を挿入",
            Submenu::Recent => "最近使ったファイル",
            Submenu::TabWidth => "タブ幅",
            Submenu::LineEnding => "改行コードを指定して保存",
            Submenu::Format => "書式",
            Submenu::Table => "表",
        }
    }
}

/// メニューの 1 項目。
#[derive(Debug, Clone)]
pub enum Item {
    /// 押せる項目
    Action {
        label: String,
        /// 併記する打鍵（`Ctrl + S` など）。無ければ空。
        ///
        /// **キー割り当ての表から作る**（v2.1.0 R-10）。利用者が変えたら変わる
        accel: String,
        message: Message,
        enabled: bool,
        /// いま選ばれている状態か（表示モードなど）
        checked: bool,
        /// 折りたたみの中身か（字下げして描く）
        indent: bool,
        /// 開いている間、この文字を押すと選べる（§7.2）
        access: Option<char>,
    },
    /// 区切り線
    Separator,
    /// 押せない見出し（並びの意味を示す）
    Heading(&'static str),
    /// 折りたたみの見出し（押すと開閉する）
    Fold {
        label: &'static str,
        open: bool,
        message: Message,
        enabled: bool,
        /// 開いている間、この文字を押すと開閉する（§7.2）
        access: Option<char>,
    },
}

impl Item {
    fn action(label: impl Into<String>, accel: impl Into<String>, message: Message) -> Self {
        Item::Action {
            label: label.into(),
            accel: accel.into(),
            message,
            enabled: true,
            checked: false,
            indent: false,
            access: None,
        }
    }

    /// 開いている間に押せる文字を割り当てる（§7.2）。
    ///
    /// **同じメニューの中で重ねない。** 重ねると、先に見つかったほうしか
    /// 選べず、押しても何も起きないように見える（試験で見ている）
    fn access(mut self, key: char) -> Self {
        if let Item::Action { access, .. } = &mut self {
            *access = Some(key);
        }
        self
    }

    fn enabled(mut self, value: bool) -> Self {
        if let Item::Action { enabled, .. } = &mut self {
            *enabled = value;
        }
        self
    }

    fn checked(mut self, value: bool) -> Self {
        if let Item::Action { checked, .. } = &mut self {
            *checked = value;
        }
        self
    }

    /// 折りたたみの中身として字下げする。
    fn indented(mut self) -> Self {
        if let Item::Action { indent, .. } = &mut self {
            *indent = true;
        }
        self
    }
}

/// メニューを組み立てるのに要る、いまの状態。
///
/// **App をそのまま渡さない。** 渡すと試験のために App を作ることになり、
/// 窓が要る。
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub mode: ViewMode,
    pub toc_visible: bool,
    pub scroll_sync: bool,
    /// 出力中・ダイアログを出している最中は、ファイル操作を止める
    pub busy: bool,
    /// 何かを選んでいるか（切り取り・コピーが押せるか）
    pub has_selection: bool,
    /// いまの文書に BOM が付いているか
    pub has_bom: bool,
    /// いまの文書の文字コード（§19.4）
    pub encoding: crate::io::Encoding,
    /// いまの文書の改行コード（§19.6）
    pub line_ending: crate::io::LineEnding,
    /// 開いているファイルが在るか（開き直せるか）
    pub has_path: bool,
    /// 開いている折りたたみ。**1 つだけ開く**
    pub open_submenu: Option<Submenu>,
    /// 最近開いたファイル（新しい順。§19.7）
    pub recent: &'a [std::path::PathBuf],
    /// 空白・タブ・改行の印を出しているか（§4.11）
    pub show_invisibles: bool,
    /// 怪しい文字を強調しているか（§4.12）
    pub show_gremlins: bool,
    /// 編集中の内容を定期的に退避するか（§18.3）
    pub autosave_draft: bool,
    /// いまのタブ幅（§4.10）
    pub tab_width: usize,
    /// いまの表示倍率（1.0 = 等倍。§4.13）
    pub zoom: f32,
    pub can_undo: bool,
    pub can_redo: bool,
    /// キー割り当て（R-10）。**併記する打鍵をここから出す**
    pub keymap: &'a Keymap,
    /// 表の整形を使うか（R-16。切っていればメニューに出さない）
    pub table_format: bool,
    /// 常に最前面か（R-02）
    pub on_top: bool,
    /// 畳んでいる見出しがあるか（R-20）
    pub has_folds: bool,
}

impl Context<'_> {
    /// 操作に割り当たっている打鍵（メニューに併記する）。
    fn accel(&self, command: Command) -> String {
        self.keymap.accel(command)
    }
}

/// 1 つのメニューの中身。
pub fn items(menu: Menu, context: Context<'_>) -> Vec<Item> {
    let c = |command: Command| context.accel(command);
    match menu {
        Menu::File => vec![
            Item::action("新規", c(Command::New), Message::File(FileCommand::New))
                .enabled(!context.busy)
                .access('N'),
            // **別の窓で開く**（R-09）。窓ごとに別のプロセスになる
            Item::action(
                "新しいウィンドウ",
                c(Command::NewWindow),
                Message::NewWindow,
            )
            .access('W'),
            Item::action("開く…", c(Command::Open), Message::File(FileCommand::Open))
                .enabled(!context.busy)
                .access('O'),
            Item::action(
                "新しいウィンドウで開く…",
                c(Command::OpenInNewWindow),
                Message::OpenInNewWindow,
            )
            .enabled(!context.busy)
            .access('L'),
            // **OS のダイアログが出せない環境がある**（§14.1）。
            // 自前のものをいつでも呼べるようにしておく
            Item::action(
                "アプリ内で開く…",
                c(Command::OpenInApp),
                Message::OpenBrowser,
            )
            .enabled(!context.busy)
            .access('I'),
            Item::Fold {
                label: Submenu::Recent.label(),
                open: context.open_submenu == Some(Submenu::Recent),
                message: Message::ToggleSubmenu(Submenu::Recent),
                // **1 つも無ければ押せない。** 開いて空では何も伝わらない
                enabled: !context.recent.is_empty() && !context.busy,
                access: Some('R'),
            },
            Item::Separator,
            Item::action(
                "上書き保存",
                c(Command::Save),
                Message::File(FileCommand::Save),
            )
            .enabled(!context.busy)
            .access('S'),
            Item::action(
                "名前を付けて保存…",
                c(Command::SaveAs),
                Message::File(FileCommand::SaveAs),
            )
            .enabled(!context.busy)
            .access('A'),
            // **BOM の有無は状態表示の隣で切り替えたくなる**ので、
            // ステータスバーに出している表記（§7.4）と同じ言葉にする
            Item::action(
                "BOM を付けて保存",
                c(Command::SaveWithBom),
                Message::File(FileCommand::SaveWithBom),
            )
            .enabled(!context.busy && !context.has_bom)
            .access('B'),
            // **文字コードと改行コードは 1 か所で選ぶ**（R-04）。
            // v2.0 の 3 つの折りたたみをまとめた
            Item::action(
                "文字コード・改行コード…",
                c(Command::Encoding),
                Message::OpenEncodingDialog,
            )
            .enabled(!context.busy)
            .access('E'),
            Item::Separator,
            Item::action(
                "PDF に出力…",
                c(Command::ExportPdf),
                Message::File(FileCommand::ExportPdf),
            )
            .enabled(!context.busy)
            .access('P'),
            Item::action(
                "HTML に出力…",
                c(Command::ExportHtml),
                Message::File(FileCommand::ExportHtml),
            )
            .enabled(!context.busy)
            .access('H'),
            Item::Separator,
            // **頻繁に切り替えないものは設定画面へ移した**（R-03）
            Item::action("設定…", c(Command::Settings), Message::OpenSettings).access('T'),
            Item::Separator,
            Item::action("終了", "Alt + F4", Message::CloseRequested).access('X'),
        ],

        Menu::Edit => {
            let mut items = vec![
                Item::action("取り消し", c(Command::Undo), Message::Undo)
                    .enabled(context.can_undo)
                    .access('U'),
                Item::action("やり直し", c(Command::Redo), Message::Redo)
                    .enabled(context.can_redo)
                    .access('R'),
                Item::Separator,
                Item::action("切り取り", c(Command::Cut), Message::Cut)
                    .enabled(context.has_selection)
                    .access('T'),
                Item::action("コピー", c(Command::Copy), Message::Copy)
                    .enabled(context.has_selection)
                    .access('C'),
                // **貼り付けはいつでも押せる。** クリップボードの中身は
                // 読みに行くまで分からず、毎回覗くと他のアプリの邪魔になる
                Item::action("貼り付け", c(Command::Paste), Message::Paste).access('P'),
                Item::action("すべて選択", c(Command::SelectAll), Message::SelectAll).access('A'),
                Item::Separator,
                Item::action(
                    "行の複製",
                    c(Command::DuplicateLine),
                    Message::DuplicateLine,
                )
                .access('D'),
                Item::action("行の削除", c(Command::DeleteLine), Message::DeleteLine).access('L'),
                Item::action("行の連結", c(Command::JoinLines), Message::JoinLines).access('J'),
                Item::action("字下げ", "Tab", Message::Indent(true))
                    .enabled(context.has_selection)
                    .access('I'),
                Item::action("字下げを戻す", "Shift + Tab", Message::Indent(false))
                    .enabled(context.has_selection)
                    .access('O'),
                Item::Separator,
                // コメント（R-06）。**コードの中ならその言語の書き方**
                Item::action(
                    "行コメントの切替",
                    c(Command::LineComment),
                    Message::ToggleComment { block: false },
                )
                .access('M'),
                Item::action(
                    "ブロックコメントの切替",
                    c(Command::BlockComment),
                    Message::ToggleComment { block: true },
                )
                .access('K'),
                Item::Fold {
                    label: Submenu::Format.label(),
                    open: context.open_submenu == Some(Submenu::Format),
                    message: Message::ToggleSubmenu(Submenu::Format),
                    enabled: true,
                    access: Some('S'),
                },
            ];
            // **表の整形を切っていたら出さない**（R-16）
            if context.table_format {
                items.push(Item::Fold {
                    label: Submenu::Table.label(),
                    open: context.open_submenu == Some(Submenu::Table),
                    message: Message::ToggleSubmenu(Submenu::Table),
                    enabled: true,
                    access: Some('E'),
                });
            }
            items.extend([
                Item::Separator,
                // **変換は選んだ範囲だけに効く。** 文書全体へ効くと、
                // 押し間違いを取り消すまで気づけない
                Item::Fold {
                    label: Submenu::Transform.label(),
                    open: context.open_submenu == Some(Submenu::Transform),
                    message: Message::ToggleSubmenu(Submenu::Transform),
                    enabled: context.has_selection,
                    access: Some('V'),
                },
                Item::Fold {
                    label: Submenu::Insert.label(),
                    open: context.open_submenu == Some(Submenu::Insert),
                    message: Message::ToggleSubmenu(Submenu::Insert),
                    enabled: true,
                    access: Some('N'),
                },
                Item::Separator,
                Item::action("検索・置換…", c(Command::Find), Message::OpenSearch).access('F'),
            ]);
            items
        }

        Menu::View => vec![
            Item::action(
                "編集",
                c(Command::ViewEdit),
                Message::SetMode(ViewMode::Edit),
            )
            .checked(context.mode == ViewMode::Edit)
            .access('E'),
            Item::action(
                "プレビュー",
                c(Command::ViewPreview),
                Message::SetMode(ViewMode::Preview),
            )
            .checked(context.mode == ViewMode::Preview)
            .access('P'),
            Item::action(
                "分割",
                c(Command::ViewSplit),
                Message::SetMode(ViewMode::Split),
            )
            .checked(context.mode == ViewMode::Split)
            .access('S'),
            Item::Separator,
            Item::action("目次", c(Command::ToggleToc), Message::ToggleToc)
                .checked(context.toc_visible)
                .access('T'),
            // **分割のときだけ効く**（片方しか見えていないなら相手がいない）
            Item::action(
                "スクロール同期",
                c(Command::ToggleSync),
                Message::ToggleSync,
            )
            .enabled(context.mode == ViewMode::Split)
            .checked(context.scroll_sync)
            .access('Y'),
            Item::Separator,
            Item::action(
                "空白・タブ・改行を表示",
                c(Command::ToggleInvisibles),
                Message::ToggleInvisibles,
            )
            .checked(context.show_invisibles)
            .access('W'),
            Item::Separator,
            // **倍率は数で出す。** 「拡大」だけでは、いまどこに居るのか分からない
            Item::action("拡大", c(Command::ZoomIn), Message::ZoomIn)
                .enabled(context.zoom < super::ZOOM_MAX)
                .access('I'),
            Item::action("縮小", c(Command::ZoomOut), Message::ZoomOut)
                .enabled(context.zoom > super::ZOOM_MIN)
                .access('O'),
            Item::action(
                format!(
                    "等倍に戻す（いま {}%）",
                    (context.zoom * 100.0).round() as i32
                ),
                c(Command::ZoomReset),
                Message::ZoomReset,
            )
            .enabled((context.zoom - 1.0).abs() > f32::EPSILON)
            .access('R'),
            Item::Separator,
            // その場で切り替える（R-02）。起動時の扱いは設定画面で決める
            Item::action(
                "常に最前面に表示",
                c(Command::AlwaysOnTop),
                Message::ToggleAlwaysOnTop,
            )
            .checked(context.on_top)
            .access('F'),
        ],

        // 移動（R-07 / R-18 / R-19 / R-20）
        Menu::Go => vec![
            Item::Heading("定義と参照"),
            Item::action(
                "定義へ移動",
                c(Command::Definition),
                Message::Seek(crate::app::Seek::Definition),
            )
            .access('D'),
            Item::action(
                "型定義へ移動",
                c(Command::TypeDefinition),
                Message::Seek(crate::app::Seek::TypeDefinition),
            )
            .access('T'),
            Item::action(
                "宣言へ移動",
                c(Command::Declaration),
                Message::Seek(crate::app::Seek::Declaration),
            )
            .access('C'),
            Item::action(
                "実装へ移動",
                c(Command::Implementation),
                Message::Seek(crate::app::Seek::Implementation),
            )
            .access('I'),
            Item::action(
                "参照を探す",
                c(Command::References),
                Message::Seek(crate::app::Seek::References),
            )
            .access('R'),
            Item::Separator,
            Item::action(
                "対応する括弧へ",
                c(Command::MatchBracket),
                Message::MatchBracket,
            )
            .access('B'),
            Item::action(
                "閉じ括弧へ移動",
                c(Command::ClosingBracket),
                Message::ClosingBracket,
            )
            .access('K'),
            Item::action("指定行へジャンプ…", c(Command::GotoLine), Message::OpenGoto).access('G'),
            Item::Separator,
            Item::action(
                "前の見出しへ",
                c(Command::PreviousHeading),
                Message::HeadingStep(false),
            )
            .access('P'),
            Item::action(
                "次の見出しへ",
                c(Command::NextHeading),
                Message::HeadingStep(true),
            )
            .access('N'),
            Item::action(
                "見出しへ移動…",
                c(Command::GotoHeading),
                Message::OpenHeadingPicker,
            )
            .access('H'),
            Item::Separator,
            Item::action(
                "リンクを開く",
                c(Command::OpenLink),
                Message::OpenLinkAtCaret,
            )
            .access('L'),
            Item::action(
                "リンク切れを検査",
                c(Command::CheckLinks),
                Message::CheckLinks,
            )
            .access('E'),
            Item::Separator,
            Item::action("この見出しを畳む", c(Command::Fold), Message::Fold).access('F'),
            Item::action("この見出しを開く", c(Command::Unfold), Message::Unfold)
                .enabled(context.has_folds)
                .access('O'),
            Item::action("すべて畳む", c(Command::FoldAll), Message::FoldAll).access('A'),
            Item::action("すべて開く", c(Command::UnfoldAll), Message::UnfoldAll)
                .enabled(context.has_folds)
                .access('U'),
        ],

        Menu::Help => vec![Item::action(
            "このアプリについて…",
            context.accel(Command::About),
            Message::OpenAbout,
        )
        .access('A')],
    }
}

/// 保存するときに選べる組み合わせ（§19.4）。
///
/// **BOM の有無まで含めて並べる。** 「UTF-8」と「UTF-8N」を別の項目に
/// するのは TeraPad と同じで、選んだものがそのまま結果になる
fn save_choices() -> Vec<(&'static str, crate::io::Encoding, bool)> {
    use crate::io::Encoding;
    vec![
        ("UTF-8（BOM あり）", Encoding::Utf8, true),
        ("UTF-8（BOM なし）", Encoding::Utf8, false),
        ("Shift_JIS", Encoding::ShiftJis, false),
        ("EUC-JP", Encoding::EucJp, false),
        ("JIS", Encoding::Iso2022Jp, false),
        ("UTF-16LE（BOM あり）", Encoding::Utf16Le, true),
        ("UTF-16BE（BOM あり）", Encoding::Utf16Be, true),
    ]
}

/// 折りたたみの中身（§19.6）。
fn submenu_items(which: Submenu, context: Context<'_>) -> Vec<Item> {
    match which {
        Submenu::ReopenAs => crate::io::Encoding::ALL
            .iter()
            .map(|encoding| {
                Item::action(encoding.label(), "", Message::ReopenAs(*encoding))
                    .enabled(context.has_path && !context.busy)
                    .checked(context.encoding == *encoding)
                    .indented()
            })
            .collect(),
        Submenu::Transform => crate::edit::transform::Transform::ALL
            .iter()
            .map(|which| {
                Item::action(
                    which.label(),
                    context.accel(Command::Transform(*which)),
                    Message::Transform(*which),
                )
                .enabled(context.has_selection)
                .indented()
            })
            .collect(),
        Submenu::LineEnding => crate::io::LineEnding::ALL
            .iter()
            .map(|ending| {
                Item::action(ending.label(), "", Message::SaveWithLineEnding(*ending))
                    .enabled(!context.busy)
                    .checked(context.line_ending == *ending)
                    .indented()
            })
            .collect(),
        Submenu::TabWidth => TAB_WIDTHS
            .iter()
            .map(|width| {
                Item::action(format!("{width} 桁"), "", Message::SetTabWidth(*width))
                    .checked(context.tab_width == *width)
                    .indented()
            })
            .collect(),
        Submenu::Recent => {
            let mut items: Vec<Item> = context
                .recent
                .iter()
                .map(|path| {
                    // **見えるのは名前だけにする。** 長い共有フォルダのパスは
                    // メニューの幅を超えて読めない
                    let label = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    Item::action(label, "", Message::OpenRecent(path.clone()))
                        .enabled(!context.busy)
                        .indented()
                })
                .collect();
            // **消す口を末尾に置く**（R-12）
            items.push(Item::action("一覧を消す", "", Message::ClearRecent).indented());
            items
        }
        Submenu::Format => {
            let c = |command: Command| context.accel(command);
            vec![
                Item::action(
                    "太字",
                    c(Command::Bold),
                    Message::Format(crate::app::FormatKind::Bold),
                )
                .indented(),
                Item::action(
                    "斜体",
                    c(Command::Italic),
                    Message::Format(crate::app::FormatKind::Italic),
                )
                .indented(),
                Item::action(
                    "インラインコード",
                    c(Command::InlineCode),
                    Message::Format(crate::app::FormatKind::Code),
                )
                .indented(),
                Item::action(
                    "リンク",
                    c(Command::Link),
                    Message::Format(crate::app::FormatKind::Link),
                )
                .indented(),
            ]
        }
        Submenu::Table => vec![
            Item::action(
                "表を整形",
                context.accel(Command::FormatTable),
                Message::FormatTable,
            )
            .indented(),
            Item::action(
                "表に行を足す",
                context.accel(Command::TableAddRow),
                Message::TableAddRow,
            )
            .indented(),
            Item::action(
                "表に列を足す",
                context.accel(Command::TableAddColumn),
                Message::TableAddColumn,
            )
            .indented(),
        ],
        Submenu::Insert => crate::edit::datetime::Stamp::ALL
            .iter()
            .map(|stamp| {
                Item::action(
                    stamp.label(),
                    context.accel(Command::Insert(*stamp)),
                    Message::InsertStamp(*stamp),
                )
                .indented()
            })
            .collect(),
        Submenu::SaveWithEncoding => save_choices()
            .into_iter()
            .map(|(label, encoding, has_bom)| {
                Item::action(label, "", Message::SaveWithEncoding(encoding, has_bom))
                    .enabled(!context.busy)
                    // いまの形と同じものに印を付ける
                    .checked(
                        context.encoding == encoding
                            && (!encoding.supports_bom() || context.has_bom == has_bom),
                    )
                    .indented()
            })
            .collect(),
    }
}

/// 割り当て文字から、押せる項目のメッセージを探す（§7.2）。
///
/// **押せない項目は選ばない。** 押せるように見えて何も起きないほうが
/// 分かりにくい。大文字小文字は区別しない。
pub fn find_access(menu: Menu, context: Context<'_>, key: char) -> Option<Message> {
    let key = key.to_ascii_uppercase();
    expand(items(menu, context), context)
        .into_iter()
        .find_map(|item| match item {
            Item::Action {
                message,
                enabled: true,
                access: Some(found),
                ..
            }
            | Item::Fold {
                message,
                enabled: true,
                access: Some(found),
                ..
            } if found.to_ascii_uppercase() == key => Some(message),
            _ => None,
        })
}

/// 折りたたみを開いた状態を反映した並びにする。
///
/// **開いているものの直後へ差し込む。** 別の場所へ出すと、
/// どの見出しの中身なのか分からない
pub fn expand(items: Vec<Item>, context: Context<'_>) -> Vec<Item> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let fold = match &item {
            Item::Fold { open: true, .. } => context.open_submenu,
            _ => None,
        };
        out.push(item);
        if let Some(which) = fold {
            out.extend(submenu_items(which, context));
        }
    }
    out
}

/// 選べるタブ幅（§4.10）。**TeraPad と同じ並び**
const TAB_WIDTHS: [usize; 4] = [1, 2, 4, 8];

/// 出力の形式（メニューから渡す用）。
pub fn format_of(command: FileCommand) -> Option<Format> {
    match command {
        FileCommand::ExportPdf => Some(Format::Pdf),
        FileCommand::ExportHtml => Some(Format::Html),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keymap() -> &'static Keymap {
        static KEYMAP: std::sync::OnceLock<Keymap> = std::sync::OnceLock::new();
        KEYMAP.get_or_init(|| Keymap::new(&std::collections::BTreeMap::new()))
    }

    fn context() -> Context<'static> {
        Context {
            mode: ViewMode::Split,
            toc_visible: true,
            scroll_sync: true,
            busy: false,
            has_selection: true,
            has_bom: false,
            encoding: crate::io::Encoding::Utf8,
            line_ending: crate::io::LineEnding::Lf,
            has_path: true,
            open_submenu: None,
            recent: &[],
            show_invisibles: false,
            show_gremlins: true,
            autosave_draft: true,
            tab_width: 4,
            zoom: 1.0,
            can_undo: true,
            can_redo: true,
            keymap: keymap(),
            table_format: true,
            on_top: false,
            has_folds: true,
        }
    }

    fn labels(menu: Menu, context: Context<'_>) -> Vec<String> {
        items(menu, context)
            .into_iter()
            .filter_map(|item| match item {
                Item::Action { label, .. } => Some(label),
                Item::Separator | Item::Heading(_) | Item::Fold { .. } => None,
            })
            .collect()
    }

    fn find(menu: Menu, context: Context<'_>, label: &str) -> Item {
        items(menu, context)
            .into_iter()
            .find(|item| matches!(item, Item::Action { label: l, .. } if l == label))
            .unwrap_or_else(|| panic!("{label} が無い"))
    }

    fn is_enabled(item: &Item) -> bool {
        matches!(item, Item::Action { enabled: true, .. })
    }

    fn is_checked(item: &Item) -> bool {
        matches!(item, Item::Action { checked: true, .. })
    }

    /// そのメニューで割り当てられている文字を集める。
    fn access_keys(menu: Menu, context: Context<'_>) -> Vec<char> {
        expand(items(menu, context), context)
            .into_iter()
            .filter_map(|item| match item {
                Item::Action { access, .. } | Item::Fold { access, .. } => access,
                _ => None,
            })
            .collect()
    }

    /// **同じメニューの中で重ねない**（§7.2）。
    ///
    /// 重ねると、先に見つかったほうしか選べず、
    /// 押しても何も起きないように見える
    #[test]
    fn access_keys_are_unique_within_a_menu() {
        for menu in Menu::ALL {
            // 折りたたみを開いた状態でも重ならないことを見る
            for open in [
                None,
                Some(Submenu::Recent),
                Some(Submenu::ReopenAs),
                Some(Submenu::SaveWithEncoding),
                Some(Submenu::Transform),
                Some(Submenu::Insert),
                Some(Submenu::TabWidth),
                Some(Submenu::Format),
                Some(Submenu::Table),
            ] {
                let context = Context {
                    open_submenu: open,
                    ..context()
                };
                let keys = access_keys(menu, context);
                let mut seen = keys.clone();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), keys.len(), "{} で重複: {keys:?}", menu.label());
            }
        }
    }

    /// **見出しの割り当ても重ねない。**
    #[test]
    fn the_menu_bar_access_keys_are_unique() {
        let mut keys: Vec<char> = Menu::ALL.iter().map(|m| m.access()).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "見出しの割り当てが重なっている");
    }

    /// 割り当て文字から項目を引ける（大文字小文字は区別しない）。
    #[test]
    fn an_access_key_finds_its_item() {
        for key in ['n', 'N'] {
            assert!(
                matches!(
                    find_access(Menu::File, context(), key),
                    Some(Message::File(FileCommand::New))
                ),
                "{key} で新規が引けない"
            );
        }
    }

    /// **押せない項目は選ばない。** 押せるように見えて何も起きないほうが分かりにくい
    #[test]
    fn a_disabled_item_is_not_found() {
        let busy = Context {
            busy: true,
            ..context()
        };
        assert!(find_access(Menu::File, busy, 'n').is_none());
    }

    /// 割り当ての無い文字では何も起きない。
    #[test]
    fn an_unassigned_key_finds_nothing() {
        assert!(find_access(Menu::File, context(), '9').is_none());
    }

    /// 見出しが並ぶ（§7.2）。
    #[test]
    fn the_menu_bar_lists_every_menu() {
        assert_eq!(Menu::ALL.len(), 5);
        assert_eq!(Menu::File.label(), "ファイル");
    }

    /// **文字コードと改行コードは 1 つのダイアログにまとめた**（R-04）。
    #[test]
    fn the_encodings_are_one_dialog() {
        let folds: Vec<&'static str> = items(Menu::File, context())
            .iter()
            .filter_map(|item| match item {
                Item::Fold { label, .. } => Some(*label),
                _ => None,
            })
            .collect();
        assert_eq!(folds, [Submenu::Recent.label()]);
        assert!(labels(Menu::File, context())
            .iter()
            .any(|l| l == "文字コード・改行コード…"));
    }

    /// **併記する打鍵は割り当ての表から出る**（R-10）。
    #[test]
    fn accelerators_come_from_the_keymap() {
        match find(Menu::File, context(), "上書き保存") {
            Item::Action { accel, .. } => assert_eq!(accel, "Ctrl + S"),
            other => panic!("{other:?}"),
        }
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert("save".to_owned(), "Ctrl+Alt+S".to_owned());
        let changed = Keymap::new(&overrides);
        let context = Context {
            keymap: &changed,
            ..context()
        };
        match find(Menu::File, context, "上書き保存") {
            Item::Action { accel, .. } => assert_eq!(accel, "Ctrl + Alt + S"),
            other => panic!("{other:?}"),
        }
    }

    /// **表の整形を切ったらメニューから消える**（R-16）。
    #[test]
    fn the_table_menu_follows_the_setting() {
        let has_table = |context: Context<'_>| {
            items(Menu::Edit, context).iter().any(
                |item| matches!(item, Item::Fold { label, .. } if *label == Submenu::Table.label()),
            )
        };
        assert!(has_table(context()));
        assert!(!has_table(Context {
            table_format: false,
            ..context()
        }));
    }

    /// 移動のメニューに定義・参照・見出し・折りたたみが並ぶ（R-07 / R-18 / R-20）。
    #[test]
    fn the_go_menu_has_the_navigation() {
        let labels = labels(Menu::Go, context());
        for wanted in [
            "定義へ移動",
            "型定義へ移動",
            "宣言へ移動",
            "実装へ移動",
            "参照を探す",
            "閉じ括弧へ移動",
            "前の見出しへ",
            "リンク切れを検査",
            "すべて畳む",
        ] {
            assert!(labels.iter().any(|l| l == wanted), "{wanted} が無い");
        }
    }

    /// **頻繁に切り替えないものは設定画面へ移した**（R-03）。
    #[test]
    fn rarely_changed_items_moved_to_settings() {
        let view = labels(Menu::View, context());
        assert!(!view.iter().any(|l| l.starts_with("外観")));
        assert!(!view.iter().any(|l| l == "怪しい文字を強調"));
        let file = labels(Menu::File, context());
        assert!(!file.iter().any(|l| l == "異常終了に備えて退避する"));
        assert!(file.iter().any(|l| l == "設定…"));
    }

    /// 最近使ったファイルの末尾に「一覧を消す」がある（R-12）。
    #[test]
    fn the_recent_list_can_be_cleared() {
        let recent = [std::path::PathBuf::from("C:/a.md")];
        let open = Context {
            recent: &recent,
            open_submenu: Some(Submenu::Recent),
            ..context()
        };
        let shown = expand(items(Menu::File, open), open);
        assert!(shown.iter().any(|item| matches!(
            item,
            Item::Action {
                message: Message::ClearRecent,
                ..
            }
        )));
    }

    /// ファイルの操作が一通り入る。
    #[test]
    fn the_file_menu_covers_the_file_operations() {
        let labels = labels(Menu::File, context());
        for part in ["新規", "開く…", "上書き保存", "PDF に出力…", "終了"] {
            assert!(
                labels.iter().any(|l| l == part),
                "{part} が無い: {labels:?}"
            );
        }
    }

    /// **出力中はファイル操作を止める。** 同じ先へ二重に書かない
    #[test]
    fn file_actions_are_disabled_while_busy() {
        let busy = Context {
            busy: true,
            ..context()
        };
        assert!(!is_enabled(&find(Menu::File, busy, "開く…")));
        // 終了は止めない（確認は別に出る）
        assert!(is_enabled(&find(Menu::File, busy, "終了")));
    }

    /// いま選んでいるものに印が付く。
    #[test]
    fn the_current_mode_is_checked() {
        let context = Context {
            mode: ViewMode::Preview,
            ..context()
        };
        assert!(is_checked(&find(Menu::View, context, "プレビュー")));
        assert!(!is_checked(&find(Menu::View, context, "編集")));
    }

    /// **スクロール同期は分割のときだけ効く**（§16.14）。
    #[test]
    fn scroll_sync_is_only_available_in_split() {
        let split = context();
        assert!(is_enabled(&find(Menu::View, split, "スクロール同期")));

        let edit = Context {
            mode: ViewMode::Edit,
            ..context()
        };
        assert!(!is_enabled(&find(Menu::View, edit, "スクロール同期")));
    }

    /// **戻せないときは押せない。** 押せるのに何も起きないほうが分かりにくい
    #[test]
    fn undo_is_disabled_when_there_is_nothing_to_undo() {
        let empty = Context {
            can_undo: false,
            can_redo: false,
            ..context()
        };
        assert!(!is_enabled(&find(Menu::Edit, empty, "取り消し")));
        assert!(!is_enabled(&find(Menu::Edit, empty, "やり直し")));
        // 検索はいつでも押せる
        assert!(is_enabled(&find(Menu::Edit, empty, "検索・置換…")));
    }

    /// **選んでいないときは切り取り・コピーが押せない。**
    /// 押せるのに何も起きないほうが分かりにくい
    #[test]
    fn cut_and_copy_need_a_selection() {
        let nothing = Context {
            has_selection: false,
            ..context()
        };
        assert!(!is_enabled(&find(Menu::Edit, nothing, "切り取り")));
        assert!(!is_enabled(&find(Menu::Edit, nothing, "コピー")));
        // 貼り付けは中身が分からないので止めない
        assert!(is_enabled(&find(Menu::Edit, nothing, "貼り付け")));
    }

    /// 戻せるときは押せる。
    #[test]
    fn undo_is_enabled_when_there_is_something_to_undo() {
        assert!(is_enabled(&find(Menu::Edit, context(), "取り消し")));
    }

    /// **すでに BOM が付いていたら押せない。**
    /// 押しても何も変わらないのに「付けた」と思わせない
    #[test]
    fn adding_a_bom_is_offered_only_when_there_is_none() {
        assert!(is_enabled(&find(Menu::File, context(), "BOM を付けて保存")));

        let with_bom = Context {
            has_bom: true,
            ..context()
        };
        assert!(!is_enabled(&find(Menu::File, with_bom, "BOM を付けて保存")));
    }
}
