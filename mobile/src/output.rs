//! PDF・HTML の出力（v3 R-02）。中身は PC 版と同じ土台の書き出し

use mdview_core::document::Document;
use mdview_core::export::range::ExportRange;
use mdview_core::export::{html, pdf, Silent};
use mdview_core::layout::face_measure::FaceMeasurer;

use crate::platform::Format;

/// 出力のバイト列
pub fn export(text: &str, title: &str, format: Format) -> Result<Vec<u8>, String> {
    let document = Document::from_text(text.to_owned());
    match format {
        Format::Html => html::export(&document, title, None, &Silent)
            .map(|out| out.text.into_bytes())
            .map_err(|error| format!("HTML にできませんでした: {error:?}")),
        // 字の幅は同梱の字体で測る（PC 版は画面の部品で測る）
        Format::Pdf => {
            let measurer = FaceMeasurer::new(pdf::BODY_SIZE).ok_or("同梱の字体を読めません")?;
            pdf::export(&document, &measurer, title, None, &Silent, ExportRange::All)
                .map_err(|error| format!("PDF にできませんでした: {error:?}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_formats_come_out() {
        let text = "# 題\n\n本文と **太字**。\n\n- [ ] 作業\n";
        let pdf = export(text, "題", Format::Pdf).expect("PDF");
        assert!(pdf.starts_with(b"%PDF-"));
        let html = String::from_utf8(export(text, "題", Format::Html).expect("HTML")).unwrap();
        assert!(
            html.contains("<h1") && html.contains("<strong>太字</strong>"),
            "{html}"
        );
    }
}
