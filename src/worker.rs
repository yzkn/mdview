//! 別の糸で走らせた仕事の後始末。
//!
//! **落ちた理由を画面へ出すための層である。** 糸が落ちると、結果を返す口
//! （チャネル）が閉じるだけで、呼び出し側には「途中で終わった」ことしか
//! 分からない。実際に出力で踏んだ（§10.30）。
//!
//! ```text
//! krilla が落ちる → 出力の糸が死ぬ → 送り口が閉じる
//!   → 画面には「出力が中断されました」とだけ出る
//! ```
//!
//! 何が起きたのかを知るのに、試験で再現させるまで一往復かかった。
//! **理由はその場で拾って、そのまま人へ渡す。**

/// 別の糸で走らせる仕事を包み、落ちたら理由を返す。
///
/// `what` は「何をしていたか」。利用者が読む文に入るので、
/// 「PDF の出力」のように名詞で渡す。
///
/// **`AssertUnwindSafe` を使う理由**: 包む相手はどれも「落ちたらその結果を
/// 捨てる」ものだけである。落ちた後に触る状態が無いので、途中まで書き換わった
/// 値を後から読む心配がない。
pub fn catch<T>(what: &str, task: impl FnOnce() -> T) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(task)) {
        Ok(value) => Ok(value),
        Err(panic) => Err(format!("{what}が異常終了しました: {}", reason(&panic))),
    }
}

/// 落下の理由を文字にする。
///
/// **握りつぶさない。** 型が分からないときも「理由が取れなかった」と出す。
/// 空文字にすると、画面には何も出ないのと同じになる
fn reason(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = panic.downcast_ref::<String>() {
        return text.clone();
    }
    "理由を取れませんでした".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_result_passes_through() {
        assert_eq!(catch("計算", || 1 + 1), Ok(2));
    }

    /// **落ちた理由が文に入る**（§10.30 で困ったところ）。
    #[test]
    fn the_panic_message_is_kept() {
        let error = catch("PDF の出力", || panic!("ページ 2 へ飛べません"))
            .expect_err("落ちたのに成功になっている");
        assert!(error.contains("ページ 2 へ飛べません"), "{error}");
        assert!(error.contains("PDF の出力"), "{error}");
    }

    /// `String` で落ちても拾う（`format!` を使った panic）。
    #[test]
    fn a_string_payload_is_kept() {
        let error = catch("出力", || panic!("{}", format!("鍵 {} が無い", 3)))
            .expect_err("落ちたのに成功になっている");
        assert!(error.contains("鍵 3 が無い"), "{error}");
    }

    /// 文字でない理由でも、何か出す。**黙らない**
    #[test]
    fn an_unknown_payload_still_says_something() {
        let error =
            catch("出力", || std::panic::panic_any(42_u8)).expect_err("落ちたのに成功になっている");
        assert!(error.contains("理由を取れませんでした"), "{error}");
    }

    /// **呼び出し側は生き続ける。** 落ちたあとも次の仕事ができる
    #[test]
    fn the_caller_survives() {
        let _ = catch("一度目", || panic!("落ちる"));
        assert_eq!(catch("二度目", || "無事"), Ok("無事"));
    }
}
