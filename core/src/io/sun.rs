//! 日の出・日の入り（v2.3.0 R-06）。
//!
//! **ネットにつながず計算で求める**（要件定義書 §0.2）。式は日の出の式（NOAA の太陽位置の
//! 近似と同じもの）。大気の屈折と太陽の半径を見込んで、太陽の中心が地平線の 0.833° 下に
//! あるときを日の出・日の入りとする。誤差は 1〜2 分。
//!
//! 緯度は北を正、経度は東を正（東京は 35.6895, 139.6917）。

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

/// 東京（都庁）。**既定の場所**
pub const TOKYO: (f64, f64) = (35.6895, 139.6917);

/// その日の日の出・日の入り。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SunTimes {
    Normal {
        rise: DateTime<Utc>,
        set: DateTime<Utc>,
    },
    /// 一日中出ている（白夜）
    AlwaysUp,
    /// 一日中出ない（極夜）
    AlwaysDown,
}

/// ユリウス日を時刻にする
fn from_julian(julian: f64) -> DateTime<Utc> {
    let seconds = (julian - 2_440_587.5) * 86_400.0;
    Utc.timestamp_opt(seconds.floor() as i64, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

/// その日（`date`。その土地の暦の日）の日の出・日の入り
pub fn times(date: NaiveDate, latitude: f64, longitude: f64) -> SunTimes {
    let to_rad = f64::to_radians;
    let to_deg = f64::to_degrees;
    // その日の正午（UTC）のユリウス日から、2000 年 1 月 1 日正午からの日数
    let days = (date - NaiveDate::from_ymd_opt(1970, 1, 1).expect("ある")).num_days() as f64;
    let julian_noon = 2_440_587.5 + days + 0.5;
    let n = (julian_noon - 2_451_545.0 + 0.0008).round();
    // 平均太陽時
    let mean_solar = n - longitude / 360.0;
    // 平均近点角・中心差・黄経
    let anomaly = (357.5291 + 0.985_600_28 * mean_solar).rem_euclid(360.0);
    let center = 1.9148 * to_rad(anomaly).sin()
        + 0.0200 * to_rad(2.0 * anomaly).sin()
        + 0.0003 * to_rad(3.0 * anomaly).sin();
    let ecliptic = (anomaly + center + 180.0 + 102.9372).rem_euclid(360.0);
    // 南中
    let transit = 2_451_545.0 + mean_solar + 0.0053 * to_rad(anomaly).sin()
        - 0.0069 * to_rad(2.0 * ecliptic).sin();
    // 赤緯
    let declination = (to_rad(ecliptic).sin() * to_rad(23.4397).sin()).asin();
    // 時角
    let latitude = to_rad(latitude);
    let cos_hour = (to_rad(-0.833).sin() - latitude.sin() * declination.sin())
        / (latitude.cos() * declination.cos());
    if cos_hour < -1.0 {
        return SunTimes::AlwaysUp;
    }
    if cos_hour > 1.0 {
        return SunTimes::AlwaysDown;
    }
    let hour = to_deg(cos_hour.acos());
    SunTimes::Normal {
        rise: from_julian(transit - hour / 360.0),
        set: from_julian(transit + hour / 360.0),
    }
}

/// いまが明るい時間か。**日の出と日の入りに、分単位のずらしを足して比べる**
///
/// `date` はその土地の今日（`now` の暦の日）。白夜なら明るい、極夜なら暗い
pub fn is_day(
    now: DateTime<Utc>,
    date: NaiveDate,
    latitude: f64,
    longitude: f64,
    rise_offset: i32,
    set_offset: i32,
) -> bool {
    match times(date, latitude, longitude) {
        SunTimes::Normal { rise, set } => {
            let rise = rise + chrono::Duration::minutes(rise_offset.into());
            let set = set + chrono::Duration::minutes(set_offset.into());
            rise <= now && now < set
        }
        SunTimes::AlwaysUp => true,
        SunTimes::AlwaysDown => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日本時間の時:分
    fn jst(time: DateTime<Utc>) -> (u32, u32) {
        use chrono::Timelike;
        let local = time + chrono::Duration::hours(9);
        (local.hour(), local.minute())
    }

    fn near(actual: (u32, u32), expected: (u32, u32)) -> bool {
        let minutes = |(h, m): (u32, u32)| (h * 60 + m) as i32;
        (minutes(actual) - minutes(expected)).abs() <= 3
    }

    /// 国立天文台の暦（東京）と 3 分以内で合う
    #[test]
    fn tokyo_matches_the_almanac() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap();
        let SunTimes::Normal { rise, set } = times(date, TOKYO.0, TOKYO.1) else {
            panic!("東京では日が昇る");
        };
        assert!(near(jst(rise), (4, 25)), "夏至の日の出: {:?}", jst(rise));
        assert!(near(jst(set), (19, 0)), "夏至の日の入り: {:?}", jst(set));

        let date = NaiveDate::from_ymd_opt(2026, 12, 22).unwrap();
        let SunTimes::Normal { rise, set } = times(date, TOKYO.0, TOKYO.1) else {
            panic!("東京では日が昇る");
        };
        assert!(near(jst(rise), (6, 47)), "冬至の日の出: {:?}", jst(rise));
        assert!(near(jst(set), (16, 32)), "冬至の日の入り: {:?}", jst(set));
    }

    #[test]
    fn polar_days_and_nights() {
        let summer = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap();
        assert_eq!(
            times(summer, 78.2, 15.6),
            SunTimes::AlwaysUp,
            "スバールバルの夏"
        );
        let winter = NaiveDate::from_ymd_opt(2026, 12, 21).unwrap();
        assert_eq!(times(winter, 78.2, 15.6), SunTimes::AlwaysDown);
    }

    #[test]
    fn offsets_shift_the_boundaries() {
        let date = NaiveDate::from_ymd_opt(2026, 12, 22).unwrap();
        let SunTimes::Normal { set, .. } = times(date, TOKYO.0, TOKYO.1) else {
            panic!();
        };
        let just_after = set + chrono::Duration::minutes(5);
        assert!(
            !is_day(just_after, date, TOKYO.0, TOKYO.1, 0, 0),
            "日の入りの後は暗い"
        );
        assert!(
            is_day(just_after, date, TOKYO.0, TOKYO.1, 0, 30),
            "日の入りを 30 分遅らせれば、まだ明るい"
        );
        let noon = Utc.with_ymd_and_hms(2026, 12, 22, 3, 0, 0).unwrap();
        assert!(is_day(noon, date, TOKYO.0, TOKYO.1, 0, 0));
    }
}
