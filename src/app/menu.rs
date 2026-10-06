//! メニューの中身（§7.2）。
//!
//! **並びと有効・無効だけをここで決める。** 画面の組み立ては持たないので、
//! 窓無しで試験できる。
//!
//! **自前で持つ理由**（§7.2）: Windows / Linux では OS のメニューを使わず
//! 自前で描く。アプリが直接キーを受けるため、v1 で起きた
//! 「Windows でアクセラレータが発火しない」が構造的に起きない。

use crate::render::{FileCommand, Message, ViewMode};
use crate::{app::Format, io::settings::ThemePreference};

/// 見出し（メニューバーに並ぶもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    View,
    Help,
}

impl Menu {
    pub const ALL: [Menu; 4] = [Menu::File, Menu::Edit, Menu::View, Menu::Help];

    /// `Alt` と合わせて押す文字（§7.2）。
    ///
    /// **Windows の作法に合わせる。** 画面には `ファイル(F)` のように出し、
    /// 割り当て文字へ下線を引く
    pub fn access(self) -> char {
        match self {
            Menu::File => 'F',
            Menu::Edit => 'E',
            Menu::View => 'V',
            Menu::Help => 'H',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Menu::File => "ファイル",
            Menu::Edit => "編集",
            Menu::View => "表示",
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
        }
    }
}

/// メニューの 1 項目。
#[derive(Debug, Clone)]
pub enum Item {
    /// 押せる項目
    Action {
        label: String,
        /// 併記する打鍵（`Ctrl + S` など）。無ければ空
        accel: &'static str,
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
    fn action(label: impl Into<String>, accel: &'static str, message: Message) -> Self {
        Item::Action {
            label: label.into(),
            accel,
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Context<'a> {
    pub mode: ViewMode,
    pub theme: ThemePreference,
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
}

/// 1 つのメニューの中身。
pub fn items(menu: Menu, context: Context<'_>) -> Vec<Item> {
    match menu {
        Menu::File => vec![
            Item::action("新規", "Ctrl + N", Message::File(FileCommand::New))
                .enabled(!context.busy)
                .access('N'),
            Item::action("開く…", "Ctrl + O", Message::File(FileCommand::Open))
                .enabled(!context.busy)
                .access('O'),
            // **OS のダイアログが出せない環境がある**（§14.1）。
            // 自前のものをいつでも呼べるようにしておく
            Item::action("アプリ内で開く…", "", Message::OpenBrowser)
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
            Item::action("上書き保存", "Ctrl + S", Message::File(FileCommand::Save))
                .enabled(!context.busy)
                .access('S'),
            Item::action(
                "名前を付けて保存…",
                "Ctrl + Shift + S",
                Message::File(FileCommand::SaveAs),
            )
            .enabled(!context.busy)
            .access('A'),
            // **BOM の有無は状態表示の隣で切り替えたくなる**ので、
            // ステータスバーに出している表記（§7.4）と同じ言葉にする
            Item::action(
                "BOM を付けて保存",
                "",
                Message::File(FileCommand::SaveWithBom),
            )
            .enabled(!context.busy && !context.has_bom)
            .access('B'),
            // **既定で入れておく。** 失うほうが痛い（§18.3）
            Item::action("異常終了に備えて退避する", "", Message::ToggleAutosaveDraft)
                .checked(context.autosave_draft)
                .access('D'),
            Item::Separator,
            Item::Fold {
                label: Submenu::ReopenAs.label(),
                open: context.open_submenu == Some(Submenu::ReopenAs),
                message: Message::ToggleSubmenu(Submenu::ReopenAs),
                // **保存先の無い文書は開き直せない。** 読む元が無い
                enabled: context.has_path && !context.busy,
                access: Some('E'),
            },
            Item::Fold {
                label: Submenu::SaveWithEncoding.label(),
                open: context.open_submenu == Some(Submenu::SaveWithEncoding),
                message: Message::ToggleSubmenu(Submenu::SaveWithEncoding),
                enabled: !context.busy,
                access: Some('C'),
            },
            Item::Fold {
                label: Submenu::LineEnding.label(),
                open: context.open_submenu == Some(Submenu::LineEnding),
                message: Message::ToggleSubmenu(Submenu::LineEnding),
                enabled: !context.busy,
                access: Some('K'),
            },
            Item::Separator,
            Item::action("PDF に出力…", "", Message::File(FileCommand::ExportPdf))
                .enabled(!context.busy)
                .access('P'),
            Item::action("HTML に出力…", "", Message::File(FileCommand::ExportHtml))
                .enabled(!context.busy)
                .access('H'),
            Item::Separator,
            Item::action("終了", "Alt + F4", Message::CloseRequested).access('X'),
        ],

        // **選択と切り取り・貼り付けは未実装**（A-1 の残り）。
        // 押せない項目を並べると、出来ているのか壊れているのか分からないので置かない
        Menu::Edit => vec![
            Item::action("取り消し", "Ctrl + Z", Message::Undo)
                .enabled(context.can_undo)
                .access('U'),
            Item::action("やり直し", "Ctrl + Y", Message::Redo)
                .enabled(context.can_redo)
                .access('R'),
            Item::Separator,
            Item::action("切り取り", "Ctrl + X", Message::Cut)
                .enabled(context.has_selection)
                .access('T'),
            Item::action("コピー", "Ctrl + C", Message::Copy)
                .enabled(context.has_selection)
                .access('C'),
            // **貼り付けはいつでも押せる。** クリップボードの中身は
            // 読みに行くまで分からず、毎回覗くと他のアプリの邪魔になる
            Item::action("貼り付け", "Ctrl + V", Message::Paste).access('P'),
            Item::action("すべて選択", "Ctrl + A", Message::SelectAll).access('A'),
            Item::Separator,
            Item::action("行の複製", "Ctrl + D", Message::DuplicateLine).access('D'),
            Item::action("行の削除", "Ctrl + L", Message::DeleteLine).access('L'),
            Item::action("行の連結", "Ctrl + J", Message::JoinLines).access('J'),
            Item::action("字下げ", "Tab", Message::Indent(true))
                .enabled(context.has_selection)
                .access('I'),
            Item::action("字下げを戻す", "Shift + Tab", Message::Indent(false))
                .enabled(context.has_selection)
                .access('O'),
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
            Item::action("指定行へジャンプ…", "Ctrl + G", Message::OpenGoto).access('G'),
            Item::action("対応する括弧へ", "Ctrl + ]", Message::MatchBracket).access('B'),
            Item::action("検索・置換…", "Ctrl + F", Message::OpenSearch).access('F'),
        ],

        Menu::View => vec![
            Item::action("編集", "", Message::SetMode(ViewMode::Edit))
                .checked(context.mode == ViewMode::Edit)
                .access('E'),
            Item::action("プレビュー", "", Message::SetMode(ViewMode::Preview))
                .checked(context.mode == ViewMode::Preview)
                .access('P'),
            Item::action("分割", "", Message::SetMode(ViewMode::Split))
                .checked(context.mode == ViewMode::Split)
                .access('S'),
            Item::Separator,
            Item::action("目次", "", Message::ToggleToc)
                .checked(context.toc_visible)
                .access('T'),
            // **分割のときだけ効く**（片方しか見えていないなら相手がいない）
            Item::action("スクロール同期", "", Message::ToggleSync)
                .enabled(context.mode == ViewMode::Split)
                .checked(context.scroll_sync)
                .access('Y'),
            Item::Separator,
            Item::action("空白・タブ・改行を表示", "", Message::ToggleInvisibles)
                .checked(context.show_invisibles)
                .access('W'),
            // **既定で出す。** 貼り付けで紛れ込むものを、気づく前に保存させない
            Item::action("怪しい文字を強調", "", Message::ToggleGremlins)
                .checked(context.show_gremlins)
                .access('M'),
            Item::Fold {
                label: Submenu::TabWidth.label(),
                open: context.open_submenu == Some(Submenu::TabWidth),
                message: Message::ToggleSubmenu(Submenu::TabWidth),
                enabled: true,
                access: Some('B'),
            },
            Item::Separator,
            // **倍率は数で出す。** 「拡大」だけでは、いまどこに居るのか分からない
            Item::action("拡大", "Ctrl + +", Message::ZoomIn)
                .enabled(context.zoom < super::ZOOM_MAX)
                .access('I'),
            Item::action("縮小", "Ctrl + -", Message::ZoomOut)
                .enabled(context.zoom > super::ZOOM_MIN)
                .access('O'),
            Item::action(
                format!(
                    "等倍に戻す（いま {}%）",
                    (context.zoom * 100.0).round() as i32
                ),
                "Ctrl + 0",
                Message::ZoomReset,
            )
            .enabled((context.zoom - 1.0).abs() > f32::EPSILON)
            .access('R'),
            Item::Separator,
            Item::action(
                "外観: 明るい",
                "",
                Message::SetTheme(ThemePreference::Light),
            )
            .checked(context.theme == ThemePreference::Light)
            .access('L'),
            Item::action("外観: 暗い", "", Message::SetTheme(ThemePreference::Dark))
                .checked(context.theme == ThemePreference::Dark)
                .access('K'),
            Item::action(
                "外観: OS に合わせる",
                "",
                Message::SetTheme(ThemePreference::System),
            )
            .checked(context.theme == ThemePreference::System)
            .access('Z'),
        ],

        Menu::Help => vec![Item::action("このアプリについて…", "", Message::OpenAbout).access('A')],
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
                Item::action(which.label(), "", Message::Transform(*which))
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
        Submenu::Recent => context
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
            .collect(),
        Submenu::Insert => crate::edit::datetime::Stamp::ALL
            .iter()
            .map(|stamp| Item::action(stamp.label(), "", Message::InsertStamp(*stamp)).indented())
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

    fn context() -> Context<'static> {
        Context {
            mode: ViewMode::Split,
            theme: ThemePreference::System,
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
        assert_eq!(Menu::ALL.len(), 4);
        assert_eq!(Menu::File.label(), "ファイル");
    }

    /// **文字コードと改行コードはファイルメニューの中にある**（利用者の要望）。
    #[test]
    fn the_encodings_live_under_the_file_menu() {
        let folds: Vec<&'static str> = items(Menu::File, context())
            .iter()
            .filter_map(|item| match item {
                Item::Fold { label, .. } => Some(*label),
                _ => None,
            })
            .collect();
        assert_eq!(
            folds,
            [
                Submenu::Recent.label(),
                Submenu::ReopenAs.label(),
                Submenu::SaveWithEncoding.label(),
                Submenu::LineEnding.label()
            ]
        );
    }

    /// **閉じているうちは中身を出さない**（メニューが窓の高さを超える）。
    #[test]
    fn a_closed_fold_hides_its_items() {
        let closed = context();
        let shown = expand(items(Menu::File, closed), closed);
        assert!(
            !shown.iter().any(|item| matches!(
                item,
                Item::Action {
                    message: Message::ReopenAs(_),
                    ..
                }
            )),
            "閉じているのに中身が出ている"
        );
    }

    /// **開き直しと保存が両方選べる**（§19.6）。
    #[test]
    fn the_folds_offer_reopen_and_save() {
        for (which, wanted) in [
            (Submenu::ReopenAs, "Shift_JIS"),
            (Submenu::SaveWithEncoding, "UTF-8（BOM なし）"),
        ] {
            let open = Context {
                open_submenu: Some(which),
                ..context()
            };
            let labels: Vec<String> = expand(items(Menu::File, open), open)
                .into_iter()
                .filter_map(|item| match item {
                    Item::Action { label, .. } => Some(label),
                    _ => None,
                })
                .collect();
            assert!(
                labels.iter().any(|l| l == wanted),
                "{wanted} が無い: {labels:?}"
            );
        }
    }

    /// **保存先の無い文書は開き直せない。** 読む元が無い
    #[test]
    fn an_unsaved_document_cannot_be_reopened() {
        let fresh = Context {
            has_path: false,
            open_submenu: Some(Submenu::ReopenAs),
            ..context()
        };
        let items = expand(items(Menu::File, fresh), fresh);
        for item in &items {
            if let Item::Action {
                message, enabled, ..
            } = item
            {
                if matches!(message, Message::ReopenAs(_)) {
                    assert!(!enabled, "開き直せることになっている: {item:?}");
                }
            }
        }
        // 見出しそのものも押せない
        assert!(items
            .iter()
            .any(|item| matches!(item, Item::Fold { enabled: false, .. })));
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

    /// 外観は 3 つのうち 1 つだけに印が付く。
    #[test]
    fn exactly_one_theme_is_checked() {
        let context = Context {
            theme: ThemePreference::Dark,
            ..context()
        };
        let checked = items(Menu::View, context)
            .iter()
            .filter(|item| {
                matches!(item, Item::Action { label, checked: true, .. } if label.starts_with("外観"))
            })
            .count();
        assert_eq!(checked, 1);
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
