//! 文字コードの判定と変換（利用者の要望。TeraPad に準じる）。
//!
//! **iced を知らない。** 窓無しで試験できる（§4.2）。
//!
//! 扱うのは日本語の文書で実際に出会うものに限る。
//!
//! | 表記 | 中身 |
//! |---|---|
//! | UTF-8 | BOM の有無は [`super::FileFormat::has_bom`] が持つ |
//! | Shift_JIS | Windows-31J（CP932）。`encoding_rs` の `SHIFT_JIS` がこれ |
//! | EUC-JP | 補助漢字（`0x8F` 始まり）も読む |
//! | JIS | ISO-2022-JP。エスケープで切り替える |
//! | UTF-16LE / BE | BOM 付きが普通だが、無いものも見る |
//!
//! **新規の文書は BOM 付き UTF-8** とする（§19.4）。既に在るファイルは
//! 開いたときの文字コードをそのまま保存に戻す。開いて保存しただけで
//! 全行に差分が出るのは受け入れられないためである。

/// 文字コード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    ShiftJis,
    EucJp,
    /// ISO-2022-JP
    Iso2022Jp,
    Utf16Le,
    Utf16Be,
}

/// UTF-8 の BOM。
pub const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
/// UTF-16LE の BOM。
pub const UTF16LE_BOM: &[u8] = &[0xFF, 0xFE];
/// UTF-16BE の BOM。
pub const UTF16BE_BOM: &[u8] = &[0xFE, 0xFF];

impl Encoding {
    /// 画面に出す並び（ファイルメニューの並び順）。
    pub const ALL: [Encoding; 6] = [
        Encoding::Utf8,
        Encoding::ShiftJis,
        Encoding::EucJp,
        Encoding::Iso2022Jp,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
    ];

    /// 画面に出す名前。**ステータスバーとメニューで同じ言葉を使う**。
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::ShiftJis => "Shift_JIS",
            Encoding::EucJp => "EUC-JP",
            Encoding::Iso2022Jp => "JIS",
            Encoding::Utf16Le => "UTF-16LE",
            Encoding::Utf16Be => "UTF-16BE",
        }
    }

    /// BOM を付けられるか。
    ///
    /// **Shift_JIS・EUC-JP・JIS には BOM が無い。** 付けられない文字コードで
    /// 「BOM あり」と表示すると、付いていると思わせる
    pub fn supports_bom(self) -> bool {
        matches!(self, Encoding::Utf8 | Encoding::Utf16Le | Encoding::Utf16Be)
    }

    /// この文字コードの BOM。
    pub fn bom(self) -> Option<&'static [u8]> {
        match self {
            Encoding::Utf8 => Some(UTF8_BOM),
            Encoding::Utf16Le => Some(UTF16LE_BOM),
            Encoding::Utf16Be => Some(UTF16BE_BOM),
            _ => None,
        }
    }

    fn codec(self) -> &'static encoding_rs::Encoding {
        match self {
            Encoding::Utf8 => encoding_rs::UTF_8,
            Encoding::ShiftJis => encoding_rs::SHIFT_JIS,
            Encoding::EucJp => encoding_rs::EUC_JP,
            Encoding::Iso2022Jp => encoding_rs::ISO_2022_JP,
            Encoding::Utf16Le => encoding_rs::UTF_16LE,
            Encoding::Utf16Be => encoding_rs::UTF_16BE,
        }
    }
}

/// 判定の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    pub encoding: Encoding,
    pub has_bom: bool,
}

/// 文字コードを判定する。
///
/// **順番に意味がある。**
///
///   1. BOM があればそれで決まる（迷う余地が無い）
///   2. エスケープ列があれば JIS（他の文字コードには現れない）
///   3. `NUL` が規則正しく並んでいれば BOM 無しの UTF-16
///   4. UTF-8 として妥当なら UTF-8（ASCII だけの文書もここに入る）
///   5. 残りを Shift_JIS と EUC-JP で採点して選ぶ
///
/// **必ず何かを返す。** 判定に失敗して開けないより、開いてから
/// 指定し直せるほうがよい（§19.4）
pub fn detect(bytes: &[u8]) -> Detected {
    if bytes.starts_with(UTF8_BOM) {
        return Detected {
            encoding: Encoding::Utf8,
            has_bom: true,
        };
    }
    // **UTF-16LE を先に見る。** `FF FE` と `FE FF` は互いに前方一致しないが、
    // 並べる順で迷わないように、広く使われるほうを先に置く
    if bytes.starts_with(UTF16LE_BOM) {
        return Detected {
            encoding: Encoding::Utf16Le,
            has_bom: true,
        };
    }
    if bytes.starts_with(UTF16BE_BOM) {
        return Detected {
            encoding: Encoding::Utf16Be,
            has_bom: true,
        };
    }

    if looks_like_iso2022jp(bytes) {
        return Detected {
            encoding: Encoding::Iso2022Jp,
            has_bom: false,
        };
    }

    if let Some(encoding) = looks_like_utf16(bytes) {
        return Detected {
            encoding,
            has_bom: false,
        };
    }

    if std::str::from_utf8(bytes).is_ok() {
        return Detected {
            encoding: Encoding::Utf8,
            has_bom: false,
        };
    }

    Detected {
        encoding: japanese_guess(bytes),
        has_bom: false,
    }
}

/// JIS のエスケープ列が含まれるか。
///
/// **他の文字コードには現れない並びである。** `ESC` に続く 2〜3 バイトで
/// 文字集合を切り替える
fn looks_like_iso2022jp(bytes: &[u8]) -> bool {
    const SEQUENCES: [&[u8]; 6] = [
        b"\x1b$@",  // JIS X 0208-1978
        b"\x1b$B",  // JIS X 0208-1983
        b"\x1b$(D", // JIS X 0212
        b"\x1b(B",  // ASCII へ戻る
        b"\x1b(J",  // JIS X 0201 ローマ字
        b"\x1b(I",  // 半角カナ
    ];
    bytes
        .windows(4)
        .any(|window| SEQUENCES.iter().any(|seq| window.starts_with(seq)))
}

/// BOM の無い UTF-16 か。
///
/// **`NUL` の位置で決める。** ASCII をそのまま UTF-16 にすると、
/// 偶数側（LE）か奇数側（BE）のどちらかに `NUL` が並ぶ。
/// 普通のテキストに `NUL` は出てこないため、混同しない
fn looks_like_utf16(bytes: &[u8]) -> Option<Encoding> {
    // **短すぎるものは判定しない。** たまたま `NUL` が 1 つあるだけで
    // UTF-16 と決めつけると、壊れた 1 バイト系の文書を開けなくなる
    if bytes.len() < 16 {
        return None;
    }
    let head = &bytes[..bytes.len().min(4096) & !1];

    let mut even = 0usize;
    let mut odd = 0usize;
    for (index, byte) in head.iter().enumerate() {
        if *byte == 0 {
            if index % 2 == 0 {
                even += 1;
            } else {
                odd += 1;
            }
        }
    }

    let pairs = head.len() / 2;
    // 半分以上が `NUL` で埋まっている側を採る
    let threshold = pairs / 2;
    match (odd > threshold, even > threshold) {
        // 下位・上位の順なので、`NUL` が奇数側にあるのが LE
        (true, false) => Some(Encoding::Utf16Le),
        (false, true) => Some(Encoding::Utf16Be),
        _ => None,
    }
}

/// Shift_JIS と EUC-JP を採点して選ぶ。
///
/// **辞書も統計表も持たない。** 並びとして成り立つかを数え、
/// 決まらなければ「ひらがなに見えるもの」の多いほうを採る。
/// 日本語の文書ならひらがなが最も多く出るためである。
///
/// どちらも成り立つときは **Shift_JIS** を採る（Windows の既定）。
fn japanese_guess(bytes: &[u8]) -> Encoding {
    let sjis = score_shift_jis(bytes);
    let euc = score_euc_jp(bytes);

    if sjis.errors != euc.errors {
        return if sjis.errors < euc.errors {
            Encoding::ShiftJis
        } else {
            Encoding::EucJp
        };
    }
    if sjis.kana != euc.kana {
        return if sjis.kana > euc.kana {
            Encoding::ShiftJis
        } else {
            Encoding::EucJp
        };
    }
    Encoding::ShiftJis
}

/// 採点の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Score {
    /// 並びとして成り立たなかったバイト数
    errors: usize,
    /// ひらがなに見えた文字数
    kana: usize,
}

fn score_shift_jis(bytes: &[u8]) -> Score {
    let mut score = Score { errors: 0, kana: 0 };
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            // ASCII と半角カナは 1 バイト
            0x00..=0x7F | 0xA1..=0xDF => index += 1,
            0x81..=0x9F | 0xE0..=0xEF => match bytes.get(index + 1) {
                Some(&trail) if matches!(trail, 0x40..=0x7E | 0x80..=0xFC) => {
                    // ひらがなは 0x829F〜0x82F1
                    if byte == 0x82 && (0x9F..=0xF1).contains(&trail) {
                        score.kana += 1;
                    }
                    index += 2;
                }
                _ => {
                    score.errors += 1;
                    index += 1;
                }
            },
            _ => {
                score.errors += 1;
                index += 1;
            }
        }
    }
    score
}

fn score_euc_jp(bytes: &[u8]) -> Score {
    let mut score = Score { errors: 0, kana: 0 };
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            0x00..=0x7F => index += 1,
            // 半角カナ
            0x8E => match bytes.get(index + 1) {
                Some(&trail) if (0xA1..=0xDF).contains(&trail) => index += 2,
                _ => {
                    score.errors += 1;
                    index += 1;
                }
            },
            // 補助漢字
            0x8F => match (bytes.get(index + 1), bytes.get(index + 2)) {
                (Some(&a), Some(&b)) if in_euc(a) && in_euc(b) => index += 3,
                _ => {
                    score.errors += 1;
                    index += 1;
                }
            },
            0xA1..=0xFE => match bytes.get(index + 1) {
                Some(&trail) if in_euc(trail) => {
                    // ひらがなは 0xA4A1〜0xA4F3
                    if byte == 0xA4 {
                        score.kana += 1;
                    }
                    index += 2;
                }
                _ => {
                    score.errors += 1;
                    index += 1;
                }
            },
            _ => {
                score.errors += 1;
                index += 1;
            }
        }
    }
    score
}

fn in_euc(byte: u8) -> bool {
    (0xA1..=0xFE).contains(&byte)
}

/// 読み取った本文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// **読めなかった文字があったか。** あれば画面で知らせる
    ///
    /// 黙って `` に置き換えると、保存したときに壊れた文書が残る
    pub lossy: bool,
}

/// 指定の文字コードとして読む。BOM は落とす。
pub fn decode(bytes: &[u8], encoding: Encoding) -> Decoded {
    let body = match encoding.bom() {
        Some(bom) if bytes.starts_with(bom) => &bytes[bom.len()..],
        _ => bytes,
    };

    let (text, _, lossy) = encoding.codec().decode(body);
    Decoded {
        text: text.into_owned(),
        lossy,
    }
}

/// 表せなかった文字 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unmappable {
    pub ch: char,
    /// 1 から数えた行
    pub line: usize,
    /// 1 から数えた桁（文字単位）
    pub column: usize,
}

impl std::fmt::Display for Unmappable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} 行 {} 桁 '{}'", self.line, self.column, self.ch)
    }
}

/// 探して報せる上限。
///
/// **全部は数えない。** 10MB の文書で 1 文字ずつ試すのは高くつくうえ、
/// 直すのは結局 1 つずつである。先頭のいくつかが分かれば足りる
const MAX_SAMPLES: usize = 5;

/// 書き出せなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeError {
    pub encoding: Encoding,
    /// 表せなかった文字（先頭から [`MAX_SAMPLES`] 件まで）
    pub samples: Vec<Unmappable>,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} で表せない文字があります", self.encoding.label())?;
        if self.samples.is_empty() {
            return Ok(());
        }
        let places: Vec<String> = self.samples.iter().map(|s| s.to_string()).collect();
        write!(f, ": {}", places.join("、"))?;
        if self.samples.len() >= MAX_SAMPLES {
            write!(f, " ほか")?;
        }
        Ok(())
    }
}

/// 表せない文字を探す。
///
/// **書けないと分かってから探す。** 毎回 1 文字ずつ試すと、
/// 10MB の保存が桁違いに遅くなる。
///
/// ASCII は飛ばす。扱うどの文字コードでもそのまま書けるためで、
/// ほとんどが ASCII の文書ではこれだけで大半を省ける。
fn find_unmappable(text: &str, encoding: Encoding) -> Vec<Unmappable> {
    let codec = encoding.codec();
    let mut found = Vec::new();
    let mut line = 1usize;
    let mut column = 1usize;
    let mut buffer = String::with_capacity(4);

    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
            continue;
        }
        if !ch.is_ascii() {
            buffer.clear();
            buffer.push(ch);
            let (_, _, unmappable) = codec.encode(&buffer);
            if unmappable {
                found.push(Unmappable { ch, line, column });
                if found.len() >= MAX_SAMPLES {
                    break;
                }
            }
        }
        column += 1;
    }
    found
}

/// 指定の文字コードで書き出す。
///
/// **表せない文字があったら書かない。** `encoding_rs` は表せない文字を
/// `&#12345;` のような文字参照へ逃がすが、それは**本文を書き換えている**。
/// Markdown の文書としては壊れるので、理由を返して選び直してもらう。
pub fn encode(text: &str, encoding: Encoding, has_bom: bool) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if has_bom {
        if let Some(bom) = encoding.bom() {
            out.extend_from_slice(bom);
        }
    }

    match encoding {
        // **UTF-16 は自分で並べる。** `encoding_rs` は仕様上 UTF-16 へ
        // 書き出せない（読むことはできる）
        Encoding::Utf16Le | Encoding::Utf16Be => {
            for unit in text.encode_utf16() {
                let pair = if encoding == Encoding::Utf16Le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                };
                out.extend_from_slice(&pair);
            }
        }
        _ => {
            let (bytes, _, unmappable) = encoding.codec().encode(text);
            if unmappable {
                return Err(EncodeError {
                    encoding,
                    samples: find_unmappable(text, encoding),
                });
            }
            out.extend_from_slice(&bytes);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sjis(text: &str) -> Vec<u8> {
        encode(text, Encoding::ShiftJis, false).expect("書ける")
    }

    fn euc(text: &str) -> Vec<u8> {
        encode(text, Encoding::EucJp, false).expect("書ける")
    }

    #[test]
    fn a_utf8_bom_is_recognised() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice("見出し".as_bytes());
        assert_eq!(
            detect(&bytes),
            Detected {
                encoding: Encoding::Utf8,
                has_bom: true
            }
        );
    }

    #[test]
    fn utf16_boms_are_recognised() {
        let mut le = UTF16LE_BOM.to_vec();
        le.extend_from_slice(&[0x42, 0x30]);
        assert_eq!(detect(&le).encoding, Encoding::Utf16Le);
        assert!(detect(&le).has_bom);

        let mut be = UTF16BE_BOM.to_vec();
        be.extend_from_slice(&[0x30, 0x42]);
        assert_eq!(detect(&be).encoding, Encoding::Utf16Be);
    }

    /// **BOM が無くても UTF-16 と分かる**（`NUL` の並びで決まる）。
    #[test]
    fn utf16_without_a_bom_is_found_by_the_nuls() {
        let text = "# heading of the document\n";
        let le = encode(text, Encoding::Utf16Le, false).expect("書ける");
        assert_eq!(detect(&le).encoding, Encoding::Utf16Le);
        assert!(!detect(&le).has_bom);

        let be = encode(text, Encoding::Utf16Be, false).expect("書ける");
        assert_eq!(detect(&be).encoding, Encoding::Utf16Be);
    }

    /// JIS はエスケープ列で分かる。
    #[test]
    fn iso2022jp_is_found_by_its_escapes() {
        let bytes = encode("こんにちは", Encoding::Iso2022Jp, false).expect("書ける");
        assert_eq!(detect(&bytes).encoding, Encoding::Iso2022Jp);
    }

    /// BOM の無い UTF-8。
    #[test]
    fn plain_utf8_is_recognised() {
        let bytes = "日本語の本文です。".as_bytes();
        assert_eq!(
            detect(bytes),
            Detected {
                encoding: Encoding::Utf8,
                has_bom: false
            }
        );
    }

    /// **ASCII だけの文書は UTF-8 として扱う。**
    /// どの文字コードでも同じバイト列なので、保存しても差分が出ない
    #[test]
    fn ascii_is_treated_as_utf8() {
        assert_eq!(detect(b"# heading\n\nbody\n").encoding, Encoding::Utf8);
    }

    #[test]
    fn shift_jis_is_recognised() {
        let bytes = sjis("これは日本語の文書です。\n漢字も仮名もあります。");
        assert_eq!(detect(&bytes).encoding, Encoding::ShiftJis);
    }

    #[test]
    fn euc_jp_is_recognised() {
        let bytes = euc("これは日本語の文書です。\n漢字も仮名もあります。");
        assert_eq!(detect(&bytes).encoding, Encoding::EucJp);
    }

    /// 判定して読むと元の本文に戻る。
    #[test]
    fn detected_bytes_decode_back_to_the_original() {
        let text = "# 見出し\n\n本文です。ASCII mixed 123。\n";
        for encoding in [
            Encoding::Utf8,
            Encoding::ShiftJis,
            Encoding::EucJp,
            Encoding::Iso2022Jp,
            Encoding::Utf16Le,
            Encoding::Utf16Be,
        ] {
            let bytes = encode(text, encoding, false).expect("書ける");
            let found = detect(&bytes);
            let decoded = decode(&bytes, found.encoding);
            assert_eq!(decoded.text, text, "{}", encoding.label());
            assert!(!decoded.lossy, "{}", encoding.label());
        }
    }

    /// BOM を付けて書き、読むときに落とす。
    #[test]
    fn a_bom_is_written_and_stripped() {
        for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
            let bytes = encode("本文", encoding, true).expect("書ける");
            assert!(
                bytes.starts_with(encoding.bom().expect("BOM がある")),
                "{}",
                encoding.label()
            );
            assert_eq!(
                decode(&bytes, encoding).text,
                "本文",
                "{}",
                encoding.label()
            );
        }
    }

    /// **BOM を持たない文字コードには付けない。**
    #[test]
    fn encodings_without_a_bom_get_none() {
        for encoding in [Encoding::ShiftJis, Encoding::EucJp, Encoding::Iso2022Jp] {
            assert!(!encoding.supports_bom(), "{}", encoding.label());
            assert_eq!(encoding.bom(), None);
            let bytes = encode("abc", encoding, true).expect("書ける");
            assert_eq!(bytes, b"abc", "{}", encoding.label());
        }
    }

    /// **表せない文字があったら書かない**（文字参照へ逃がさない）。
    #[test]
    fn unmappable_characters_are_refused() {
        // 絵文字は Shift_JIS に無い
        let error = encode("面白い 🙂", Encoding::ShiftJis, false).expect_err("書けない");
        assert_eq!(error.encoding, Encoding::ShiftJis);
        assert!(error.to_string().contains("Shift_JIS"));
    }

    /// **どの文字がどこにあるかを出す**（利用者の要望）。
    ///
    /// 「表せない文字があります」だけでは、10MB の文書から探せない
    #[test]
    fn the_unmappable_characters_are_located() {
        let text = "一行目\n二行目に 🙂 がある\n三行目";
        let error = encode(text, Encoding::ShiftJis, false).expect_err("書けない");

        assert_eq!(error.samples.len(), 1);
        let at = error.samples[0];
        assert_eq!(at.ch, '🙂');
        assert_eq!(at.line, 2, "行がずれている");
        assert_eq!(at.column, 6, "桁がずれている");

        let message = error.to_string();
        assert!(message.contains("2 行 6 桁"), "{message}");
        assert!(message.contains('🙂'), "{message}");
    }

    /// **数え上げは打ち切る**（10MB で 1 文字ずつ試さない）。
    #[test]
    fn the_report_stops_at_a_handful() {
        let text = "🙂".repeat(50);
        let error = encode(&text, Encoding::ShiftJis, false).expect_err("書けない");
        assert_eq!(error.samples.len(), MAX_SAMPLES);
        assert!(error.to_string().ends_with("ほか"));
    }

    /// 桁は**文字単位**で数える（バイトではない）。
    #[test]
    fn the_column_counts_characters() {
        let error = encode("日本語🙂", Encoding::ShiftJis, false).expect_err("書けない");
        assert_eq!(error.samples[0].column, 4);
    }

    /// **読めなかったことを隠さない。**
    #[test]
    fn a_wrong_encoding_reports_that_it_was_lossy() {
        let bytes = sjis("日本語");
        let decoded = decode(&bytes, Encoding::Utf8);
        assert!(decoded.lossy, "読めていないのに黙っている");
    }

    /// 短い断片で UTF-16 と決めつけない。
    #[test]
    fn a_short_fragment_is_not_called_utf16() {
        assert_ne!(detect(b"a\0b").encoding, Encoding::Utf16Le);
    }
}
