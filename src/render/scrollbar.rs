//! スクロールバーの幾何（§16.14 / §3.7）。
//!
//! **窓を知らない。** つまみの位置と長さを数で決めるところまでを持つ。
//! 描くのと掴むのは、エディタとプレビューのそれぞれが行う。
//!
//! # 単位を決めない
//!
//! エディタは**行**で巻き上げ、プレビューは**画素**で巻き上げる
//! （§3.7 のアンカー）。どちらも「全体・見えている量・いまの位置」の
//! 3 つで決まるので、**ここでは単位を問わない**。呼ぶ側が揃えて渡す。

/// バーの太さ（px）。
pub const THICKNESS: f32 = 12.0;

/// つまみの最小の長さ（px）。
///
/// **長い文書でつまみが消えないようにする。** 10MB・380,812 行では
/// 比率どおりだと 1px を割り、掴めなくなる。
pub const MIN_THUMB: f32 = 24.0;

/// つまみの位置と長さ（軌道の始点からの px）。
///
/// 全部見えているなら `None`。**そのときはバーを出さない**——
/// 動かせないものを出すと、動かせると誤解させる。
///
/// * `track` — 軌道の長さ（px）
/// * `viewport` — 見えている量（呼ぶ側の単位）
/// * `content` — 全体の量（同じ単位）
/// * `offset` — いま先頭から何進んでいるか（同じ単位）
pub fn thumb(track: f32, viewport: f32, content: f32, offset: f32) -> Option<(f32, f32)> {
    if track <= 0.0 || viewport <= 0.0 || content <= viewport {
        return None;
    }

    let ratio = (viewport / content).clamp(0.0, 1.0);
    let length = (track * ratio).max(MIN_THUMB).min(track);

    // **動かせる幅は、軌道からつまみを引いた残りである。**
    // つまみの長さを下限で伸ばした分、ここが縮む
    let travel = track - length;
    let scrollable = (content - viewport).max(f32::EPSILON);
    let position = (offset / scrollable).clamp(0.0, 1.0) * travel;

    Some((position, length))
}

/// 軌道の `at`（始点からの px）を押したときの、新しい位置。
///
/// **つまみの中心が指の下へ来る**ようにする。端は丸める。
pub fn offset_at(track: f32, viewport: f32, content: f32, at: f32) -> f32 {
    let Some((_, length)) = thumb(track, viewport, content, 0.0) else {
        return 0.0;
    };
    let travel = track - length;
    if travel <= 0.0 {
        return 0.0;
    }
    let position = (at - length / 2.0).clamp(0.0, travel);
    (position / travel) * (content - viewport).max(0.0)
}

/// つまみを `delta` px 動かしたときの、新しい位置。
///
/// 掴んで動かしている間に使う。**押した場所との差で動かす**ので、
/// つまみの中へ飛ばない。
pub fn offset_after_drag(
    track: f32,
    viewport: f32,
    content: f32,
    start_offset: f32,
    delta: f32,
) -> f32 {
    let Some((_, length)) = thumb(track, viewport, content, 0.0) else {
        return 0.0;
    };
    let travel = track - length;
    if travel <= 0.0 {
        return 0.0;
    }
    let scrollable = (content - viewport).max(0.0);
    (start_offset + delta / travel * scrollable).clamp(0.0, scrollable)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **全部見えているならバーを出さない。**
    #[test]
    fn nothing_to_scroll_means_no_bar() {
        assert_eq!(thumb(100.0, 50.0, 50.0, 0.0), None);
        assert_eq!(thumb(100.0, 50.0, 10.0, 0.0), None, "中身のほうが少ない");
    }

    /// 半分だけ見えているなら、つまみは軌道の半分。
    #[test]
    fn the_thumb_length_follows_the_visible_share() {
        let (position, length) = thumb(100.0, 50.0, 100.0, 0.0).expect("出る");
        assert_eq!(length, 50.0);
        assert_eq!(position, 0.0, "先頭では上端");
    }

    /// **末尾では下端に着く。** 1px でも余ると「まだ先がある」と見える
    #[test]
    fn the_thumb_reaches_the_end() {
        let (position, length) = thumb(100.0, 50.0, 100.0, 50.0).expect("出る");
        assert_eq!(position + length, 100.0);
    }

    /// 途中では比率どおりの位置に来る。
    #[test]
    fn the_thumb_sits_in_proportion() {
        let (position, _) = thumb(100.0, 50.0, 100.0, 25.0).expect("出る");
        assert_eq!(position, 25.0);
    }

    /// **長い文書でもつまみが消えない**（10MB で 1px を割る）。
    #[test]
    fn a_huge_document_still_has_a_grabbable_thumb() {
        let (_, length) = thumb(800.0, 40.0, 380_812.0, 0.0).expect("出る");
        assert!(length >= MIN_THUMB, "掴めない: {length}");
    }

    /// 下限まで伸ばしても、末尾では下端に着く。
    #[test]
    fn the_minimum_thumb_still_reaches_the_end() {
        let content = 380_812.0;
        let viewport = 40.0;
        let (position, length) = thumb(800.0, viewport, content, content - viewport).expect("出る");
        assert!(
            (position + length - 800.0).abs() < 0.01,
            "{position} {length}"
        );
    }

    /// 範囲の外を渡されても外へ出ない。
    #[test]
    fn an_offset_past_the_end_is_clamped() {
        let (position, length) = thumb(100.0, 50.0, 100.0, 999.0).expect("出る");
        assert_eq!(position + length, 100.0);
    }

    /// 軌道が無いときは出さない（分割を畳んだ直後など）。
    #[test]
    fn a_zero_track_is_safe() {
        assert_eq!(thumb(0.0, 50.0, 100.0, 0.0), None);
        assert_eq!(thumb(100.0, 0.0, 100.0, 0.0), None);
    }

    /// **溝を押すと、つまみの中心がそこへ来る。**
    #[test]
    fn clicking_the_track_centres_the_thumb() {
        // 軌道 100・見えている 50・全体 100 → つまみは 50、動かせるのは 50
        let offset = offset_at(100.0, 50.0, 100.0, 75.0);
        // 75 - 25 = 50 → 動かせる幅いっぱい → 末尾
        assert_eq!(offset, 50.0);
    }

    /// 端を押しても外へ出ない。
    #[test]
    fn clicking_past_the_ends_is_clamped() {
        assert_eq!(offset_at(100.0, 50.0, 100.0, -20.0), 0.0);
        assert_eq!(offset_at(100.0, 50.0, 100.0, 300.0), 50.0);
    }

    /// **掴んで動かすと、動かした分だけ進む。**
    #[test]
    fn dragging_moves_by_the_delta() {
        // 動かせる軌道 50 に対し、全体で動かせるのは 50 → 1:1
        assert_eq!(offset_after_drag(100.0, 50.0, 100.0, 0.0, 10.0), 10.0);
        assert_eq!(offset_after_drag(100.0, 50.0, 100.0, 20.0, -10.0), 10.0);
    }

    /// 掴んだまま行き過ぎても端で止まる。
    #[test]
    fn dragging_past_the_ends_is_clamped() {
        assert_eq!(offset_after_drag(100.0, 50.0, 100.0, 0.0, 999.0), 50.0);
        assert_eq!(offset_after_drag(100.0, 50.0, 100.0, 0.0, -999.0), 0.0);
    }

    /// 動かせないときは 0 のまま（掴んでも暴れない）。
    #[test]
    fn dragging_a_bar_that_is_not_there_does_nothing() {
        assert_eq!(offset_after_drag(100.0, 50.0, 50.0, 0.0, 30.0), 0.0);
        assert_eq!(offset_at(100.0, 50.0, 50.0, 30.0), 0.0);
    }
}
