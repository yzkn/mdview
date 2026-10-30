//! 使っているソフトウェアの著作権表示（v2.6.0 R-01）。About で見せる。
//!
//! 依存の一覧は `tools/build-licenses.py` が `Cargo.lock` から作り（`assets/licenses/third-party.json`）、
//! **実行ファイルに埋める**（単一の実行ファイルのまま配るため）。同梱フォントのライセンスは
//! `crate::render::fonts::LICENSES` に別に埋めてあり、一覧の頭に並べる。

use super::fluent;
use std::sync::OnceLock;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};
use serde::Deserialize;

use super::App;
use crate::render::Message;

/// 依存 1 つ
#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub license: String,
    #[serde(default)]
    pub repository: String,
    /// 全文（`ThirdParty::texts` の番号）
    pub texts: Vec<usize>,
    /// 標準文で補ったときの断り
    #[serde(default)]
    pub note: Option<String>,
}

/// 依存の一覧（同じ全文はまとめてある）
#[derive(Debug, Default, Deserialize)]
pub struct ThirdParty {
    pub packages: Vec<Package>,
    pub texts: Vec<String>,
}

/// 埋めた一覧（初めて使うときに 1 度だけ読む）
pub fn third_party() -> &'static ThirdParty {
    static DATA: OnceLock<ThirdParty> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/licenses/third-party.json"))
            .unwrap_or_default()
    })
}

/// About の一覧で選んだもの
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AboutItem {
    /// 同梱フォント（`fonts::LICENSES` の番号）
    Font(usize),
    /// 依存（`ThirdParty::packages` の番号）
    Package(usize),
}

/// About の一覧の操作
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutLicense {
    Filter(String),
    Pick(AboutItem),
}

impl App {
    pub(super) fn about_license(&mut self, message: AboutLicense) {
        match message {
            AboutLicense::Filter(value) => self.about_filter = value,
            AboutLicense::Pick(item) => self.about_pick = Some(item),
        }
    }

    /// About の「使っているソフトウェア」（絞り込みの欄・一覧・選んだものの全文）
    pub(super) fn licenses_view(&self) -> Element<'_, Message> {
        let data = third_party();
        let fonts = crate::render::fonts::LICENSES;
        let filter = self.about_filter.trim().to_lowercase();
        let wanted = |name: &str, license: &str| {
            filter.is_empty()
                || name.to_lowercase().contains(&filter)
                || license.to_lowercase().contains(&filter)
        };
        let entry = |label: String, item: AboutItem| -> Element<'_, Message> {
            let picked = self.about_pick == Some(item);
            button(text(label).size(11))
                .padding([1, 4])
                .width(Length::Fill)
                .style(if picked {
                    button::primary
                } else {
                    button::text
                })
                .on_press(Message::AboutLicense(AboutLicense::Pick(item)))
                .into()
        };
        let mut list = column![].spacing(0);
        for (index, (name, _)) in fonts.iter().enumerate() {
            if wanted(name, "OFL-1.1") {
                list = list.push(entry(
                    format!("{name} — OFL-1.1（同梱フォント）"),
                    AboutItem::Font(index),
                ));
            }
        }
        for (index, package) in data.packages.iter().enumerate() {
            if wanted(&package.name, &package.license) {
                list = list.push(entry(
                    format!("{} {} — {}", package.name, package.version, package.license),
                    AboutItem::Package(index),
                ));
            }
        }
        let body: Element<'_, Message> = match self.about_pick {
            Some(AboutItem::Font(index)) => match fonts.get(index) {
                Some((name, license)) => column![
                    text(*name).size(13),
                    text(*license).size(10).font(crate::render::fonts::mono()),
                ]
                .spacing(4)
                .into(),
                None => text("").into(),
            },
            Some(AboutItem::Package(index)) => match data.packages.get(index) {
                Some(package) => {
                    let mut page = column![text(format!(
                        "{} {}（{}）",
                        package.name, package.version, package.license
                    ))
                    .size(13)]
                    .spacing(6);
                    if !package.repository.is_empty() {
                        page = page.push(text(package.repository.as_str()).size(11));
                    }
                    if let Some(note) = &package.note {
                        page = page.push(text(note.as_str()).size(11));
                    }
                    for id in &package.texts {
                        if let Some(body) = data.texts.get(*id) {
                            page = page.push(
                                text(body.as_str())
                                    .size(10)
                                    .font(crate::render::fonts::mono()),
                            );
                        }
                    }
                    page.into()
                }
                None => text("").into(),
            },
            None => text("名前を押すと、ライセンスの全文を出します。")
                .size(11)
                .into(),
        };
        column![
            row![
                text(format!(
                    "使っているソフトウェア（依存 {} 個と同梱フォント）",
                    data.packages.len()
                ))
                .size(14),
                container(text("")).width(Length::Fill),
                fluent::text_input("名前・ライセンスで絞り込む", &self.about_filter)
                    .on_input(|value| Message::AboutLicense(AboutLicense::Filter(value)))
                    .size(12)
                    .padding([2, 6])
                    .width(Length::Fixed(200.0)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            container(fluent::scrollable(list).height(Length::Fixed(140.0)))
                .padding(2)
                .style(container::bordered_box),
            fluent::scrollable(body).height(Length::Fixed(200.0)),
        ]
        .spacing(8)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 埋めた一覧が読めて、どの依存にも全文がある
    #[test]
    fn the_embedded_list_is_complete() {
        let data = third_party();
        assert!(data.packages.len() > 100, "一覧が読めていない");
        for package in &data.packages {
            assert!(!package.texts.is_empty(), "{} に全文が無い", package.name);
            for id in &package.texts {
                assert!(
                    *id < data.texts.len(),
                    "{} の全文の番号が外れている",
                    package.name
                );
            }
        }
        // 主な依存が入っている
        for name in ["iced_core", "comrak", "ropey"] {
            assert!(
                data.packages.iter().any(|p| p.name == name),
                "{name} が無い"
            );
        }
    }

    /// 標準文で補ったものには断りがあり、著者が入っている（差し込み口が残っていない）
    #[test]
    fn supplemented_texts_name_their_authors() {
        let data = third_party();
        for package in data.packages.iter().filter(|p| p.note.is_some()) {
            for id in &package.texts {
                assert!(!data.texts[*id].contains("{copyright}"), "{}", package.name);
            }
        }
    }
}
