//! 設定画面の「自分の配色」（v2.3.0 R-05 ②）。
//!
//! 配色を作る・選んで直す・名前を変える・元にするプリセットを選ぶ・色を `#rrggbb` で書く・
//! 使う・消す。**直している配色を使っていれば、書いた色はその場で画面に出る**。

use iced::widget::{button, column, container, pick_list, row, text, text_input};
use iced::{Color, Element, Length};

use super::settings_view::{SettingChange, SettingsScreen};
use super::App;
use crate::io::palette::{self, CustomPalette, Group, KEYS};
use crate::io::settings::ThemePreference;
use crate::render::Message;

/// 自分の配色の操作。
#[derive(Debug, Clone)]
pub enum PaletteChange {
    /// 新しく作る（いまの配色を元にして、すぐに使う）
    New,
    /// 直す配色を選ぶ
    Edit(Option<String>),
    /// 名前の欄に打った
    Name(String),
    /// 打った名前に変える
    Rename,
    /// 元にするプリセット
    Base(ThemePreference),
    /// 色の欄に打った（鍵・打った文字）。**空なら元の色に戻す**
    Color(String, String),
    /// 元の色に戻す
    ResetColor(String),
    /// この配色を使う
    Use,
    /// 消す
    Delete,
}

fn message(change: PaletteChange) -> Message {
    Message::Setting(SettingChange::Palette(change))
}

impl App {
    /// 自分の配色の操作を当てる（`apply_setting` から。書き込みは呼ぶ側が済ませる）
    pub(super) fn apply_palette(&mut self, change: PaletteChange) {
        let editing = self
            .settings_screen
            .as_ref()
            .and_then(|screen| screen.palette.clone());
        match change {
            PaletteChange::New => {
                let name = palette::fresh_name(&self.settings.palettes);
                // **いまの配色を元にする**（自分の配色を使っていれば、その元）
                let base = match &self.settings.theme {
                    ThemePreference::Custom(_) => self
                        .active_palette()
                        .map_or_else(|| "system".to_owned(), |p| p.base.clone()),
                    other => other.as_str().into_owned(),
                };
                self.settings.palettes.push(CustomPalette {
                    name: name.clone(),
                    base,
                    colors: Default::default(),
                });
                self.settings.theme = ThemePreference::Custom(name.clone());
                self.edit_palette(Some(name));
            }
            PaletteChange::Edit(name) => self.edit_palette(name),
            PaletteChange::Name(typed) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.palette_name = typed;
                }
            }
            PaletteChange::Rename => {
                let (Some(old), Some(screen)) = (editing, self.settings_screen.as_ref()) else {
                    return;
                };
                let new = screen.palette_name.trim().to_owned();
                if new == old {
                    return;
                }
                let taken = self.settings.palettes.iter().any(|p| p.name == new);
                if !palette::valid_name(&new) || taken {
                    self.notice = Some(super::notice::Notice::plain(if taken {
                        format!("「{new}」はもうあります")
                    } else {
                        "名前に . = \" と改行は使えません（空も不可）".to_owned()
                    }));
                    return;
                }
                if let Some(palette) = self.settings.palettes.iter_mut().find(|p| p.name == old) {
                    palette.name.clone_from(&new);
                }
                if self.settings.theme == ThemePreference::Custom(old) {
                    self.settings.theme = ThemePreference::Custom(new.clone());
                }
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.palette = Some(new);
                }
            }
            PaletteChange::Base(base) => {
                if matches!(base, ThemePreference::Custom(_)) {
                    return;
                }
                if let Some(palette) = self.editing_palette_mut(editing.as_deref()) {
                    palette.base = base.as_str().into_owned();
                }
            }
            PaletteChange::Color(key, typed) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.palette_colors.insert(key.clone(), typed.clone());
                }
                let Some(palette) = self.editing_palette_mut(editing.as_deref()) else {
                    return;
                };
                if typed.trim().is_empty() {
                    palette.colors.remove(&key);
                } else if let Some(color) = palette::parse_hex(&typed) {
                    palette.colors.insert(key, palette::to_hex(color));
                }
            }
            PaletteChange::ResetColor(key) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.palette_colors.remove(&key);
                }
                if let Some(palette) = self.editing_palette_mut(editing.as_deref()) {
                    palette.colors.remove(&key);
                }
            }
            PaletteChange::Use => {
                if let Some(name) = editing {
                    self.settings.theme = ThemePreference::Custom(name);
                }
            }
            PaletteChange::Delete => {
                let Some(name) = editing else {
                    return;
                };
                let base = self
                    .settings
                    .palettes
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| ThemePreference::parse(&p.base));
                self.settings.palettes.retain(|p| p.name != name);
                // **使っていた配色を消したら、その元へ戻す**
                if self.settings.theme == ThemePreference::Custom(name) {
                    self.settings.theme = base.unwrap_or(ThemePreference::System);
                }
                self.edit_palette(None);
            }
        }
    }

    fn edit_palette(&mut self, name: Option<String>) {
        let colors = name
            .as_ref()
            .and_then(|name| self.settings.palettes.iter().find(|p| p.name == *name))
            .map(|p| p.colors.clone())
            .unwrap_or_default();
        if let Some(screen) = self.settings_screen.as_mut() {
            screen.palette_name = name.clone().unwrap_or_default();
            screen.palette = name;
            screen.palette_colors = colors;
        }
    }

    fn editing_palette_mut(&mut self, name: Option<&str>) -> Option<&mut CustomPalette> {
        let name = name?;
        self.settings.palettes.iter_mut().find(|p| p.name == name)
    }

    /// 設定画面の「自分の配色」の欄
    pub(super) fn palette_section<'a>(
        &'a self,
        screen: &'a SettingsScreen,
    ) -> Element<'a, Message> {
        let names: Vec<String> = self
            .settings
            .palettes
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut out = column![
            text("自分の配色").size(14),
            row![
                pick_list(names, screen.palette.clone(), |name| message(
                    PaletteChange::Edit(Some(name))
                ))
                .placeholder("直す配色を選ぶ")
                .text_size(12),
                button(text("新しく作る").size(12))
                    .padding([4, 10])
                    .on_press(message(PaletteChange::New)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            text("元にする配色を選び、変えたい色だけを #RRGGBB で書きます。空にすると元の色に戻ります。")
                .size(11),
        ]
        .spacing(8);

        let Some(palette) = screen
            .palette
            .as_ref()
            .and_then(|name| self.settings.palettes.iter().find(|p| p.name == *name))
        else {
            return out.into();
        };
        let in_use = self.settings.theme == ThemePreference::Custom(palette.name.clone());
        let presets: Vec<ThemePreference> = super::settings_view::theme_choices(&self.settings)
            .into_iter()
            .filter(|choice| !matches!(choice, ThemePreference::Custom(_) | ThemePreference::Sun))
            .collect();
        out = out
            .push(
                row![
                    text("名前").size(12).width(Length::Fixed(140.0)),
                    text_input("名前", &screen.palette_name)
                        .on_input(|v| message(PaletteChange::Name(v)))
                        .on_submit(message(PaletteChange::Rename))
                        .size(12)
                        .width(Length::Fixed(220.0)),
                    button(text("名前を変える").size(12))
                        .padding([4, 10])
                        .on_press(message(PaletteChange::Rename)),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .push(
                row![
                    text("元にする配色").size(12).width(Length::Fixed(140.0)),
                    pick_list(
                        presets,
                        Some(ThemePreference::parse(&palette.base)),
                        |base| { message(PaletteChange::Base(base)) }
                    )
                    .text_size(12),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .push(
                row![
                    button(
                        text(if in_use {
                            "使っています"
                        } else {
                            "この配色を使う"
                        })
                        .size(12)
                    )
                    .padding([4, 10])
                    .on_press_maybe((!in_use).then(|| message(PaletteChange::Use))),
                    button(text("消す").size(12))
                        .padding([4, 10])
                        .style(button::danger)
                        .on_press(message(PaletteChange::Delete)),
                ]
                .spacing(8),
            );

        // 元の色（欄が空のときの見本）
        let base_theme = self.theme_of(&ThemePreference::parse(&palette.base));
        let base_ui = base_theme.palette();
        let base_syntax = crate::render::syntax::SyntaxColors::for_text(base_ui.text);
        let base_color = |key: &str| -> Option<Color> {
            Some(match key {
                "ui.background" => base_ui.background,
                "ui.text" => base_ui.text,
                "ui.primary" => base_ui.primary,
                "editor.heading" => base_syntax.heading,
                "editor.emphasis" => base_syntax.emphasis,
                "editor.code" => base_syntax.code,
                "editor.link" => base_syntax.link,
                "editor.quote" => base_syntax.quote,
                "editor.marker" => base_syntax.marker,
                _ => return None,
            })
        };

        for group in [Group::Ui, Group::Editor, Group::Preview] {
            out = out.push(text(group.label()).size(13));
            for (key, label, _) in KEYS.iter().filter(|(_, _, g)| *g == group) {
                let key = (*key).to_owned();
                let typed = screen
                    .palette_colors
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| palette.colors.get(&key).cloned().unwrap_or_default());
                let shown = palette.color(&key).or_else(|| base_color(&key));
                let placeholder =
                    base_color(&key).map_or_else(|| "元のまま".to_owned(), palette::to_hex);
                let input_key = key.clone();
                let reset_key = key.clone();
                out = out.push(
                    row![
                        text(*label).size(12).width(Length::Fixed(260.0)),
                        text_input(&placeholder, &typed)
                            .on_input(move |v| message(PaletteChange::Color(input_key.clone(), v)))
                            .size(12)
                            .width(Length::Fixed(110.0)),
                        swatch(shown),
                        button(text("元の色").size(11))
                            .padding([2, 8])
                            .on_press_maybe(
                                palette
                                    .colors
                                    .contains_key(&key)
                                    .then(|| message(PaletteChange::ResetColor(reset_key))),
                            ),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                );
            }
        }
        out.into()
    }
}

/// 色見本。**色が分からなければ枠だけ**
fn swatch<'a>(color: Option<Color>) -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(18.0))
        .style(move |_| container::Style {
            background: color.map(Into::into),
            border: iced::Border {
                width: 1.0,
                color: Color::from_rgba(0.5, 0.5, 0.5, 0.6),
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.settings.theme = ThemePreference::Dark;
        app.open_settings();
        app
    }

    fn change(app: &mut App, change: PaletteChange) {
        let _ = app.update(message(change));
    }

    /// 作るとすぐに使い、書いた色がその場で効く
    #[test]
    fn a_new_palette_is_used_and_colors_apply() {
        let mut app = app();
        change(&mut app, PaletteChange::New);
        assert_eq!(app.settings.palettes.len(), 1);
        assert_eq!(app.settings.palettes[0].base, "dark", "いまの配色が元");
        assert_eq!(
            app.settings.theme,
            ThemePreference::Custom("自分の配色 1".to_owned())
        );
        change(
            &mut app,
            PaletteChange::Color("ui.background".into(), "#102030".into()),
        );
        assert_eq!(palette::to_hex(app.theme().palette().background), "#102030");
        change(
            &mut app,
            PaletteChange::Color("editor.heading".into(), "#ff8800".into()),
        );
        assert_eq!(
            app.syntax_colors().map(|c| palette::to_hex(c.heading)),
            Some("#ff8800".to_owned())
        );
        // 打ちかけ（読めない）は効かせないが、欄には残る
        change(
            &mut app,
            PaletteChange::Color("ui.text".into(), "#12".into()),
        );
        assert!(!app.settings.palettes[0].colors.contains_key("ui.text"));
        assert_eq!(
            app.settings_screen
                .as_ref()
                .unwrap()
                .palette_colors
                .get("ui.text")
                .map(String::as_str),
            Some("#12")
        );
        // 空にすると元の色
        change(
            &mut app,
            PaletteChange::Color("ui.background".into(), String::new()),
        );
        assert_eq!(
            app.theme().palette().background,
            iced::Theme::Dark.palette().background
        );
    }

    /// 名前を変えると、使っている指定も付いてくる。消すと元へ戻る
    #[test]
    fn renaming_and_deleting() {
        let mut app = app();
        change(&mut app, PaletteChange::New);
        change(&mut app, PaletteChange::Name("夜用".into()));
        change(&mut app, PaletteChange::Rename);
        assert_eq!(app.settings.theme, ThemePreference::Custom("夜用".into()));
        // 使えない名前は断る
        change(&mut app, PaletteChange::Name("a.b".into()));
        change(&mut app, PaletteChange::Rename);
        assert_eq!(app.settings.palettes[0].name, "夜用");
        change(&mut app, PaletteChange::Delete);
        assert!(app.settings.palettes.is_empty());
        assert_eq!(app.settings.theme, ThemePreference::Dark, "元の配色へ戻る");
    }

    /// 設定ファイルに書いて読み直しても同じ
    #[test]
    fn palettes_survive_a_round_trip() {
        let mut app = app();
        change(&mut app, PaletteChange::New);
        change(
            &mut app,
            PaletteChange::Color("preview.link".into(), "#00aa00".into()),
        );
        let read = crate::io::settings::Settings::from_toml(&app.settings.to_toml());
        assert_eq!(read.palettes, app.settings.palettes);
        assert_eq!(read.theme, app.settings.theme);
    }
}

#[cfg(test)]
mod sun_tests {
    use super::*;
    use chrono::TimeZone;

    /// **日の出・日の入りに合わせると、明るい時間と暗い時間で配色が変わる**（v2.3.0 R-06）
    #[test]
    fn the_theme_follows_the_sun() {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.settings.theme = ThemePreference::Sun;
        app.settings.sun_night_theme = ThemePreference::Custom("夜用".into());
        app.settings.palettes.push(CustomPalette {
            name: "夜用".into(),
            base: "dark".into(),
            colors: [("ui.background".to_owned(), "#000010".to_owned())]
                .into_iter()
                .collect(),
        });
        // 東京の正午（UTC 3 時）は明るい、真夜中（UTC 15 時）は暗い
        let noon = chrono::Utc.with_ymd_and_hms(2026, 12, 22, 3, 0, 0).unwrap();
        let night = chrono::Utc
            .with_ymd_and_hms(2026, 12, 22, 15, 0, 0)
            .unwrap();
        assert_eq!(app.effective_theme_at(noon), ThemePreference::Light);
        assert_eq!(
            app.effective_theme_at(night),
            ThemePreference::Custom("夜用".into()),
            "暗い時間は自分の配色も選べる"
        );
    }

    #[test]
    fn sun_settings_round_trip_and_refuse_bad_values() {
        let mut settings = crate::io::settings::Settings::default();
        assert!(settings.set_sun_field("sun_latitude", "43.0618"));
        assert!(
            !settings.set_sun_field("sun_latitude", "91"),
            "緯度は ±90 まで"
        );
        assert!(!settings.set_sun_field("sun_rise_offset", "abc"));
        assert!(settings.set_sun_field("sun_set_offset", "-45"));
        settings.theme = ThemePreference::Sun;
        let read = crate::io::settings::Settings::from_toml(&settings.to_toml());
        assert_eq!(read.sun_latitude, 43.0618);
        assert_eq!(read.sun_set_offset, -45);
        assert_eq!(read.theme, ThemePreference::Sun);
        // 明るい時間に「日の出・日の入り」は入らない
        let read = crate::io::settings::Settings::from_toml("sun_day_theme = \"sun\"\n");
        assert_eq!(read.sun_day_theme, ThemePreference::Light);
    }
}
