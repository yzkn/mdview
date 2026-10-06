//! 日付と時刻を差し込む（TeraPad の「日付/時刻の挿入」）。
//!
//! **書式はここだけで決める。** 呼び出し側は「日付」か「時刻」かを
//! 選ぶだけにして、表記の揺れを 1 か所へ閉じ込める。

/// 差し込むもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stamp {
    /// `2026-09-30`
    Date,
    /// `14:05`
    Time,
    /// `2026-09-30 14:05`
    DateTime,
}

impl Stamp {
    pub fn label(self) -> &'static str {
        match self {
            Stamp::Date => "日付を挿入",
            Stamp::Time => "時刻を挿入",
            Stamp::DateTime => "日付と時刻を挿入",
        }
    }

    pub const ALL: [Stamp; 3] = [Stamp::Date, Stamp::Time, Stamp::DateTime];

    /// 端末の時計で、いまの表記を作る。
    pub fn now(self) -> String {
        self.format(chrono::Local::now())
    }

    /// 与えられた時刻で表記を作る（試験のために分ける）。
    fn format<Tz: chrono::TimeZone>(self, at: chrono::DateTime<Tz>) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        // **ISO 8601 に寄せる。** 文書へ書くものなので、
        // 読む人の地域設定に左右されない並びにする
        match self {
            Stamp::Date => at.format("%Y-%m-%d").to_string(),
            Stamp::Time => at.format("%H:%M").to_string(),
            Stamp::DateTime => at.format("%Y-%m-%d %H:%M").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> chrono::DateTime<chrono::FixedOffset> {
        chrono::FixedOffset::east_opt(9 * 3600)
            .expect("日本の時差")
            .with_ymd_and_hms(2026, 9, 30, 14, 5, 0)
            .single()
            .expect("その時刻はある")
    }

    /// **地域設定に左右されない並びにする**（ISO 8601 に寄せる）。
    #[test]
    fn the_date_is_iso_like() {
        assert_eq!(Stamp::Date.format(at()), "2026-09-30");
    }

    #[test]
    fn the_time_is_hours_and_minutes() {
        assert_eq!(Stamp::Time.format(at()), "14:05");
    }

    #[test]
    fn both_are_separated_by_a_space() {
        assert_eq!(Stamp::DateTime.format(at()), "2026-09-30 14:05");
    }

    /// 端末の時計でも形は同じ（長さで確かめる）。
    #[test]
    fn the_local_clock_has_the_same_shape() {
        assert_eq!(Stamp::Date.now().len(), "2026-09-30".len());
        assert_eq!(Stamp::Time.now().len(), "14:05".len());
    }
}
