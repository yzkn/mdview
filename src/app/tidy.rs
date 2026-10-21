//! 検査の直しとフォーマッターを当てる（v2.5.0 R-03）。整え方は `crate::edit::format`。
//!
//! **当てるときは、検査の直し・フォーマッターのそれぞれについて設定で選ぶ**: メニューから選んだとき・ショートカットキーを
//! 押したとき・保存するとき（自分で保存したときだけ。自動保存では当てない＝打っている途中で整えない）・閉じるとき。
//! 当てた変更は 1 つの編集として履歴に入り、取り消しで戻せる。読み取り専用とテキストとして読む文書には当てない。

use super::App;
use crate::edit::format::{format, lint_fixes, FormatOptions};
use crate::io::settings::Marker;
use crate::render::{TidyTool as Tool, TidyTrigger as Trigger};

impl App {
    /// その道具を、そのときに当てる設定か
    fn tidy_enabled(&self, tool: Tool, trigger: Trigger) -> bool {
        let s = &self.settings;
        match (tool, trigger) {
            (Tool::Lint, Trigger::Menu) => s.lint_fix_menu,
            (Tool::Lint, Trigger::Key) => s.lint_fix_key,
            (Tool::Lint, Trigger::Save) => s.lint_fix_save,
            (Tool::Lint, Trigger::Close) => s.lint_fix_close,
            (Tool::Format, Trigger::Menu) => s.format_menu,
            (Tool::Format, Trigger::Key) => s.format_key,
            (Tool::Format, Trigger::Save) => s.format_save,
            (Tool::Format, Trigger::Close) => s.format_close,
        }
    }

    /// フォーマッターの整え方（設定から）
    fn format_options(&self, lint: bool) -> FormatOptions {
        let s = &self.settings;
        FormatOptions {
            lint,
            disabled: s.lint_disabled.clone(),
            tables: s.format_tables,
            bullet: s.format_bullets.then_some(s.format_bullet),
            emphasis: s
                .format_emphasis
                .then_some(match (s.emphasis_marker, s.strong_marker) {
                    (Marker::First, Marker::First) => ("*", "**"),
                    (Marker::First, Marker::Second) => ("*", "__"),
                    (Marker::Second, Marker::First) => ("_", "**"),
                    (Marker::Second, Marker::Second) => ("_", "__"),
                }),
        }
    }

    /// いまの本文に当てた結果（変わらなければ `None`）
    fn tidied(&self, lint: bool, formatter: bool) -> Option<String> {
        if self.document.is_plain() || !(lint || formatter) {
            return None;
        }
        let old = self.document.text().to_string();
        let new = if formatter {
            // **フォーマッターは検査の直しを含む**（R-03 の 1）
            format(&old, &self.format_options(true))
        } else {
            lint_fixes(&old, &self.settings.lint_disabled)
        };
        (new != old).then_some(new)
    }

    /// メニュー・ショートカットから当てる
    pub(super) fn tidy(&mut self, tool: Tool, trigger: Trigger) {
        if !self.tidy_enabled(tool, trigger) {
            let what = match tool {
                Tool::Lint => "検査の直し",
                Tool::Format => "Markdown を整える",
            };
            let how = match trigger {
                Trigger::Key => "ショートカットキー",
                _ => "メニュー",
            };
            self.notice = Some(super::notice::Notice::plain(format!(
                "{what}は、{how}からは当てない設定です（設定 → 編集補助）"
            )));
            return;
        }
        if self.refuse_edit() {
            return;
        }
        self.apply_tidy(tool == Tool::Lint, tool == Tool::Format);
    }

    /// 保存・閉じるときに当てる（設定で入れたものだけ）。**当てて変わったら `true`**
    pub(super) fn tidy_on(&mut self, trigger: Trigger) -> bool {
        if self.read_only {
            return false;
        }
        let lint = self.tidy_enabled(Tool::Lint, trigger);
        let formatter = self.tidy_enabled(Tool::Format, trigger);
        self.apply_tidy(lint, formatter)
    }

    /// 当てる（1 つの編集として履歴に入る）
    fn apply_tidy(&mut self, lint: bool, formatter: bool) -> bool {
        let Some(new) = self.tidied(lint, formatter) else {
            return false;
        };
        let old = self.document.text().to_string();
        self.replace_whole(&old, &new);
        true
    }

    /// 閉じる前（v2.5.0 R-03）。**当てて変わったら、保存先のある文書は保存する**。
    /// 当てる前から未保存だった文書は、いつもの「保存しますか」に任せる
    pub(super) fn tidy_before_exit(&mut self) {
        if self.meta.path.is_none() || self.meta.url.is_some() {
            return;
        }
        let was_dirty = self.meta.dirty;
        if self.tidy_on(Trigger::Close) && !was_dirty {
            if let Some(path) = self.meta.path.clone() {
                let _ = self.save_to(path, crate::render::SaveAs::Keep);
            }
        }
    }
}
