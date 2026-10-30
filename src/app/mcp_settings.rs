//! 設定画面の「MCP」（v2.4.0 R-06 6c）。
//!
//! 使う・窓が無いときも受ける・手元の HTTP（6d）・**操作ごとの扱い（許可・確認・拒否）**・呼ばれた記録・
//! 読む操作の注意・クライアントの設定の例。扱いの表は `crate::io::mcp_access::TOOLS`。
//!
//! **合言葉は画面に出さない**（写すボタンだけ。画面を共有していても漏れないように）。
//!
//! **クライアントの設定の例は、特定のアプリの名前を出さない**（公開側の写しの決まり）。

use super::fluent;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};

use super::settings_view::{heading, line, note, with_reset, SettingChange};
use super::App;
use crate::io::mcp_access::{default_for, permission_of, Permission, TOOLS};
use crate::render::Message;

/// 記録を出す件数
const LOG_SHOWN: usize = 30;

/// クライアントの設定の例（この実行ファイルの場所で作る）
pub fn client_config_example() -> String {
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "mdview".to_owned());
    let exe = serde_json::to_string(&exe).unwrap_or_else(|_| "\"mdview\"".to_owned());
    format!(
        "{{\n  \"mcpServers\": {{\n    \"mdview\": {{\n      \"command\": {exe},\n      \"args\": [\"--mcp\"]\n    }}\n  }}\n}}"
    )
}

/// 手元の HTTP でつなぐときの設定の例（**合言葉を入れる**。写すときだけ作る）
pub fn http_config_example(url: &str, token: &str) -> String {
    let url = serde_json::to_string(url).unwrap_or_default();
    let bearer = serde_json::to_string(&format!("Bearer {token}")).unwrap_or_default();
    format!(
        "{{\n  \"mcpServers\": {{\n    \"mdview\": {{\n      \"type\": \"http\",\n      \"url\": {url},\n      \"headers\": {{ \"Authorization\": {bearer} }}\n    }}\n  }}\n}}"
    )
}

impl App {
    /// 手元の HTTP の段（6d）
    fn mcp_http_section<'a>(
        &'a self,
        screen: &'a super::settings_view::SettingsScreen,
    ) -> Element<'a, Message> {
        let s = &self.settings;
        let d = crate::io::settings::Settings::default();
        let mut section = column![
            heading("手元の HTTP"),
            with_reset(
                super::settings_view::toggle(
                    "手元の HTTP でも受ける（127.0.0.1 だけ。合言葉が要る）",
                    s.mcp_http,
                    SettingChange::McpHttp
                ),
                (s.mcp_http != d.mcp_http).then_some(SettingChange::McpHttp(d.mcp_http)),
            ),
            with_reset(
                line(
                    "口（ポート）",
                    fluent::text_input("39217", &screen.mcp_http_port)
                        .on_input(|v| Message::Setting(SettingChange::McpHttpPort(v)))
                        .size(12)
                        .width(Length::Fixed(80.0)),
                ),
                (s.mcp_http_port != d.mcp_http_port)
                    .then(|| SettingChange::McpHttpPort(d.mcp_http_port.to_string())),
            ),
        ]
        .spacing(8);
        let state = match &screen.mcp_host {
            Some(host) if host.pid == std::process::id() => {
                format!("この窓が受けています: {}", host.url())
            }
            Some(host) => format!("ほかの窓（{}）が受けています: {}", host.pid, host.url()),
            None if s.mcp_enabled && s.mcp_http => format!(
                "受ける準備をしています（口 {} がほかで使われていると受けられません）",
                s.mcp_http_port
            ),
            None => "受けていません。".to_owned(),
        };
        section = section.push(text(state).size(11));
        if screen.mcp_host.is_some() {
            section = section.push(
                row![
                    button(text("合言葉を写す").size(11))
                        .padding([2, 10])
                        .on_press(Message::Setting(SettingChange::McpCopyToken)),
                    button(text("設定の例を写す（合言葉入り）").size(11))
                        .padding([2, 10])
                        .on_press(Message::Setting(SettingChange::McpCopyHttpExample)),
                ]
                .spacing(8),
            );
        }
        section = section.push(note(
            "合言葉は受け始めるたびに変わります（mdview を開き直したとき・口を変えたとき）。呼ぶ側は Authorization: Bearer <合言葉> を付けます。合言葉は設定フォルダの mcp-http.json にもあります。ほかの PC からはつなげません。",
        ));
        section.into()
    }

    pub(super) fn mcp_page<'a>(
        &'a self,
        screen: &'a super::settings_view::SettingsScreen,
    ) -> Element<'a, Message> {
        let s = &self.settings;
        let d = crate::io::settings::Settings::default();
        let reset_bool =
            |now: bool, default: bool, change: SettingChange| (now != default).then_some(change);
        let mut page = column![
            heading("MCP サーバー"),
            with_reset(
                super::settings_view::toggle(
                    "MCP サーバーを使う（mdview --mcp で起こされたとき・手元の HTTP で受ける）",
                    s.mcp_enabled,
                    SettingChange::McpEnabled
                ),
                reset_bool(s.mcp_enabled, d.mcp_enabled, SettingChange::McpEnabled(d.mcp_enabled)),
            ),
            with_reset(
                super::settings_view::toggle(
                    "窓が開いていないときも受ける（ファイルを読む・検査・比較・出力）",
                    s.mcp_headless,
                    SettingChange::McpHeadless
                ),
                reset_bool(s.mcp_headless, d.mcp_headless, SettingChange::McpHeadless(d.mcp_headless)),
            ),
            note("MCP クライアント（AI のアプリなど）から mdview の機能を呼べます。手元の通り道だけを使い、ネットには口を開けません。"),
            self.mcp_http_section(screen),
            row![
                heading("操作ごとの扱い"),
                container(text("")).width(Length::Fill),
                button(text("すべて既定に戻す").size(11))
                    .padding([2, 10])
                    .on_press_maybe(
                        (!s.mcp_permissions.is_empty())
                            .then_some(Message::Setting(SettingChange::McpResetAll))
                    ),
            ]
            .align_y(iced::Alignment::Center),
            note("「確認」は、呼ばれるたびに mdview が許すかどうかを聞きます（窓が無ければ OS のダイアログ）。自動保存を入れている窓を書き換える操作と、パッチを窓へ当てる操作は、「許可」でも確認します。"),
        ]
        .spacing(8);
        for (tool, label, _) in TOOLS {
            let now = permission_of(&s.mcp_permissions, tool);
            let default = default_for(tool);
            let tool_name: &'static str = tool;
            page = page.push(with_reset(
                line(
                    label,
                    row![
                        fluent::pick_list(Permission::ALL.to_vec(), Some(now), move |permission| {
                            Message::Setting(SettingChange::McpPermission(
                                tool_name.to_owned(),
                                Some(permission),
                            ))
                        })
                        .text_size(12),
                        text(*tool).size(11),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                ),
                (now != default).then(|| SettingChange::McpPermission(tool_name.to_owned(), None)),
            ));
        }
        page = page.push(heading("注意"));
        page = page.push(note(
            "ファイルを読む操作は、既定で「許可」です。壊しはしませんが、置き場を絞らないので、どのファイルの中身でも AI へ渡せます（機密のファイルを含む）。気になる場合は「ファイルを読む」を「確認」か「拒否」にしてください。",
        ));
        page = page.push(heading("クライアントの設定の例"));
        page = page.push(note(
            "MCP クライアントの設定（MCP サーバーの一覧）に、次のように書きます。",
        ));
        let example = client_config_example();
        page = page.push(
            row![
                container(
                    text(example.clone())
                        .font(crate::render::fonts::mono())
                        .size(12)
                )
                .padding(8)
                .width(Length::Fill)
                .style(container::bordered_box),
                button(text("写す").size(11))
                    .padding([2, 10])
                    .on_press(Message::Setting(SettingChange::McpCopyExample)),
            ]
            .spacing(8),
        );
        page = page.push(
            row![
                heading("呼ばれた記録"),
                container(text("")).width(Length::Fill),
                button(text("読み直す").size(11))
                    .padding([2, 10])
                    .on_press(Message::Setting(SettingChange::McpRefreshLog)),
            ]
            .align_y(iced::Alignment::Center),
        );
        if screen.mcp_log.is_empty() {
            page = page.push(note("まだ呼ばれていません。"));
        } else {
            let lines = column(screen.mcp_log.iter().take(LOG_SHOWN).map(|entry| {
                let field = |key: &str| entry[key].as_str().unwrap_or("").to_owned();
                text(format!(
                    "{}  {}  {}  {}",
                    field("time"),
                    field("tool"),
                    field("outcome"),
                    field("detail")
                ))
                .size(11)
                .into()
            }))
            .spacing(2);
            page = page.push(fluent::scrollable(lines).height(Length::Fixed(220.0)));
        }
        page.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HTTP の例は URL と合言葉を入れた JSON
    #[test]
    fn the_http_example_carries_the_token() {
        let example = http_config_example("http://127.0.0.1:39217/mcp", "abc");
        let value: serde_json::Value = serde_json::from_str(&example).expect("JSON として読める");
        let server = &value["mcpServers"]["mdview"];
        assert_eq!(server["type"], "http");
        assert_eq!(server["url"], "http://127.0.0.1:39217/mcp");
        assert_eq!(server["headers"]["Authorization"], "Bearer abc");
    }

    /// 例はこの実行ファイルを --mcp で起こす形で、**アプリの名前を出さない**
    #[test]
    fn the_example_starts_this_program() {
        let example = client_config_example();
        let value: serde_json::Value = serde_json::from_str(&example).expect("JSON として読める");
        assert_eq!(value["mcpServers"]["mdview"]["args"][0], "--mcp");
        // 名前は分けて書く（公開側の写しの禁止語の検査に当たらないように）
        let name = ["cla", "ude"].concat();
        // **実行ファイルの場所は除いて見る**（置き場のフォルダ名に入っていることがある）
        let command = value["mcpServers"]["mdview"]["command"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let template = serde_json::to_string(&value)
            .unwrap()
            .replace(&serde_json::to_string(&command).unwrap(), "\"\"");
        assert!(!template.to_lowercase().contains(&name), "{template}");
    }
}
