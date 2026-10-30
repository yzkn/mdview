//! 使っているソフトウェアの著作権表示（v3 R-03）。「その他 → 使用許諾」で見せる。
//!
//! 依存の一覧は `tools/build-licenses.py --mobile` が `mobile/Cargo.lock` から作り
//! （`assets/licenses/mobile-third-party.json`。Android・iOS の APK・.app に入る依存）、**アプリに埋める**。
//! 同梱フォントのライセンスは土台（`render::fonts::LICENSES`）にあり、一覧の頭に並べる。PC 版と同じ形

use std::sync::OnceLock;

use serde::Deserialize;

/// 依存 1 つ
#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub license: String,
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
        serde_json::from_str(include_str!(
            "../../assets/licenses/mobile-third-party.json"
        ))
        .unwrap_or_default()
    })
}

/// 一覧の 1 行（見出しと、開いたときの全文）
pub struct Entry {
    pub title: String,
    pub sub: String,
    pub body: String,
}

/// 同梱フォント → 依存の順に並べる
pub fn entries() -> Vec<Entry> {
    let fonts = mdview_core::render::fonts::LICENSES
        .iter()
        .map(|(name, text)| Entry {
            title: (*name).to_owned(),
            sub: "OFL-1.1（同梱の字体）".to_owned(),
            body: (*text).to_owned(),
        });
    let data = third_party();
    let packages = data.packages.iter().map(|package| {
        let mut body = package
            .texts
            .iter()
            .filter_map(|&index| data.texts.get(index).map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n\n----\n\n");
        if let Some(note) = &package.note {
            body = format!("（{note}）\n\n{body}");
        }
        Entry {
            title: format!("{} {}", package.name, package.version),
            sub: package.license.clone(),
            body,
        }
    });
    fonts.chain(packages).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_embedded_and_complete() {
        let data = third_party();
        assert!(data.packages.len() > 100, "一覧が読めていない");
        assert!(data.packages.iter().any(|p| p.name == "dioxus"));
        for package in &data.packages {
            assert!(!package.texts.is_empty(), "{} に全文が無い", package.name);
            assert!(package.texts.iter().all(|&i| i < data.texts.len()));
        }
        let list = entries();
        assert_eq!(list[0].title, mdview_core::render::fonts::MONO, "字体が頭");
        assert!(list.iter().all(|entry| !entry.body.is_empty()));
    }
}
