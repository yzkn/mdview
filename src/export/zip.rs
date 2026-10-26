//! ZIP を書く（v2.4.0 R-07 7e。PPTX の入れ物）。**圧縮しない**（STORED）。
//!
//! 中身は小さな XML と PNG（もう圧縮してある）なので、縮めても得が少ない。
//! 依存を増やさないため、ZIP の決まり（APPNOTE）の最小限を自前で書く。名前は ASCII だけ。

/// CRC-32（ZIP と同じ多項式 0xEDB88320）
fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (index, slot) in table.iter_mut().enumerate() {
            let mut value = index as u32;
            for _ in 0..8 {
                value = if value & 1 == 1 {
                    0xEDB8_8320 ^ (value >> 1)
                } else {
                    value >> 1
                };
            }
            *slot = value;
        }
        table
    });
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc = table[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

/// 書いている ZIP
#[derive(Default)]
pub struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Zip {
    /// 1 つ足す（`name` は ASCII の相対パス）
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let crc = crc32(data);
        let offset = self.out.len() as u32;
        let size = data.len() as u32;
        let name = name.as_bytes();
        // 日時は 1980-01-01 00:00（決めた値にして、同じ中身なら同じファイルにする）
        let (time, date) = (0u16, 0x0021u16);

        let out = &mut self.out;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // 要る版
        out.extend_from_slice(&0u16.to_le_bytes()); // 印
        out.extend_from_slice(&0u16.to_le_bytes()); // STORED
        out.extend_from_slice(&time.to_le_bytes());
        out.extend_from_slice(&date.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        let central = &mut self.central;
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // 作った版
        central.extend_from_slice(&20u16.to_le_bytes()); // 要る版
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // 付け足し
        central.extend_from_slice(&0u16.to_le_bytes()); // 注釈
        central.extend_from_slice(&0u16.to_le_bytes()); // ディスク
        central.extend_from_slice(&0u16.to_le_bytes()); // 内の属性
        central.extend_from_slice(&0u32.to_le_bytes()); // 外の属性
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
        self.count += 1;
    }

    /// 閉じて中身を返す
    pub fn finish(mut self) -> Vec<u8> {
        let start = self.out.len() as u32;
        let size = self.central.len() as u32;
        self.out.extend_from_slice(&self.central);
        let out = &mut self.out;
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_known_value() {
        // "123456789" の CRC-32 は 0xCBF43926（よく知られた検査値）
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn layout_is_a_zip() {
        let mut zip = Zip::default();
        zip.add("a.txt", b"hello");
        zip.add("dir/b.xml", b"<x/>");
        let bytes = zip.finish();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 2);
    }
}
