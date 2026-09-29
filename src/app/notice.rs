//! 画面に出す知らせ（読み込み・保存・出力の結果）。
//!
//! **失敗も成功もここに出す。** 握りつぶすと、利用者は何が起きたか分からない。
//! 出力したファイルのように「次に開きたいもの」があるときは、
//! その場から開けるようにする。

use std::path::PathBuf;

/// 知らせ 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    /// その場から開けるファイル。**出力したものだけ**を入れる
    pub link: Option<PathBuf>,
}

impl Notice {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            link: None,
        }
    }

    /// ファイルへのリンク付き。
    pub fn file(text: impl Into<String>, path: PathBuf) -> Self {
        Self {
            text: text.into(),
            link: Some(path),
        }
    }

    /// リンクに出す文字。**長いパスは真ん中を省く**。
    pub fn link_label(&self, max_chars: usize) -> Option<String> {
        self.link
            .as_ref()
            .map(|path| elide_middle(&path.display().to_string(), max_chars))
    }
}

/// 長い文字列の真ん中を省く。
///
/// **末尾を多めに残す。** パスでは末尾（ファイル名）のほうが手がかりになる。
/// 先頭を落とすと「どのドライブか」が消えるので、先頭も少しは残す。
pub fn elide_middle(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars || max_chars < 4 {
        return text.to_owned();
    }

    // 省略記号のぶんを引き、前 1 / 後ろ 2 の割合で残す
    let keep = max_chars - 1;
    let head = keep / 3;
    let tail = keep - head;

    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(elide_middle("C:/a/b.pdf", 40), "C:/a/b.pdf");
    }

    /// **省いた結果が上限に収まる。**
    #[test]
    fn elided_text_fits_the_limit() {
        let long = "C:/Users/someone/Documents/GitHub/project/samples/very-long-name.pdf";
        let elided = elide_middle(long, 40);
        assert_eq!(elided.chars().count(), 40);
        assert!(elided.contains('…'));
    }

    /// **ファイル名を残す。** 末尾が消えると、どれを出力したのか分からない
    #[test]
    fn the_file_name_survives() {
        let long = "C:/Users/someone/Documents/GitHub/project/samples/report.pdf";
        let elided = elide_middle(long, 30);
        assert!(elided.ends_with("report.pdf"), "{elided}");
        assert!(elided.starts_with("C:/U"), "先頭も残す: {elided}");
    }

    /// 日本語のパスでも文字の途中で切らない。
    #[test]
    fn multibyte_paths_are_not_cut_mid_character() {
        let long = "C:/ユーザー/書類/とても長い名前の作業フォルダー/報告書.pdf";
        let elided = elide_middle(long, 20);
        assert_eq!(elided.chars().count(), 20);
        assert!(elided.ends_with("報告書.pdf"), "{elided}");
    }

    #[test]
    fn a_plain_notice_has_no_link() {
        assert_eq!(Notice::plain("失敗しました").link_label(40), None);
    }

    #[test]
    fn a_file_notice_shows_its_path() {
        let notice = Notice::file("出力しました", PathBuf::from("C:/out/a.pdf"));
        assert_eq!(notice.link_label(40).as_deref(), Some("C:/out/a.pdf"));
    }
}
