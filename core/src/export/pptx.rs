//! PPTX を作る（v2.4.0 R-07 7e。v2.7.0 R-01 で文字を直せる形にした）。
//!
//! 中身は PresentationML の最小限: 用紙 1 つ・版下（マスターとレイアウト）1 つずつ・テーマ 1 つと、
//! スライドごとの図形（`super::pptx_slide` が作る文字の箱・表・画像）。発表者のメモはノートに入れる。
//! ここは入れ物（部品の一覧・関係・版下）だけを持ち、1 枚の中身は知らない。

use super::pptx_slide::SlideParts;

/// 埋め込む字体 1 つ（v2.8.0 R-04。EOT に包んだもの）
pub struct EmbeddedFace {
    pub typeface: &'static str,
    pub bold: bool,
    /// EOT（`.fntdata`）
    pub data: Vec<u8>,
}
use super::zip::Zip;

/// 画素から EMU へ（96 dpi で 1 px = 9525 EMU）
const EMU_PER_PX: u64 = 9525;

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const TREE_HEAD: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn rels(items: &[(String, &str, String)]) -> String {
    let mut out = format!(
        r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#
    );
    for (id, kind, target) in items {
        // リンクは外を指す
        let mode = if kind.ends_with("/hyperlink") {
            r#" TargetMode="External""#
        } else {
            ""
        };
        out.push_str(&format!(
            r#"<Relationship Id="{id}" Type="{kind}" Target="{}"{mode}/>"#,
            escape(target)
        ));
    }
    out.push_str("</Relationships>");
    out
}

/// PPTX のバイト列。`width`・`height` はスライドの論理の画素（Marp と同じ）
pub fn build(
    slides: &[SlideParts],
    width: f32,
    height: f32,
    title: &str,
    fonts: &[EmbeddedFace],
) -> Vec<u8> {
    let cx = (width.round() as u64) * EMU_PER_PX;
    let cy = (height.round() as u64) * EMU_PER_PX;
    let mut zip = Zip::default();
    let any_notes = slides.iter().any(|slide| !slide.notes.is_empty());

    // 種類の一覧
    let mut types = format!(
        r#"{HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="fntdata" ContentType="application/x-fontdata"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#
    );
    if any_notes {
        types.push_str(r#"<Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/><Override PartName="/ppt/theme/theme2.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>"#);
    }
    for (index, slide) in slides.iter().enumerate() {
        let n = index + 1;
        types.push_str(&format!(r#"<Override PartName="/ppt/slides/slide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#));
        if !slide.notes.is_empty() {
            types.push_str(&format!(r#"<Override PartName="/ppt/notesSlides/notesSlide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#));
        }
    }
    types.push_str("</Types>");
    zip.add("[Content_Types].xml", types.as_bytes());

    zip.add(
        "_rels/.rels",
        rels(&[
            ("rId1".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument", "ppt/presentation.xml".into()),
            ("rId2".into(), "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml".into()),
            ("rId3".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties", "docProps/app.xml".into()),
        ])
        .as_bytes(),
    );
    zip.add(
        "docProps/core.xml",
        format!(r#"{HEAD}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title></cp:coreProperties>"#, escape(title)).as_bytes(),
    );
    zip.add(
        "docProps/app.xml",
        format!(r#"{HEAD}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>mdview</Application><Slides>{}</Slides></Properties>"#, slides.len()).as_bytes(),
    );

    // プレゼンテーション
    let mut ids = String::new();
    let mut presentation_rels = vec![
        (
            "rId1".to_owned(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
            "slideMasters/slideMaster1.xml".to_owned(),
        ),
        (
            "rId2".to_owned(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
            "theme/theme1.xml".to_owned(),
        ),
    ];
    if any_notes {
        presentation_rels.push((
            "rId3".into(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster",
            "notesMasters/notesMaster1.xml".into(),
        ));
    }
    for index in 0..slides.len() {
        let rid = format!("rId{}", index + 10);
        ids.push_str(&format!(r#"<p:sldId id="{}" r:id="{rid}"/>"#, 256 + index));
        presentation_rels.push((
            rid,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide",
            format!("slides/slide{}.xml", index + 1),
        ));
    }
    // 埋め込む字体（v2.8.0 R-04）。書体の名前ごとに、普通と太字を並べる
    for (order, face) in fonts.iter().enumerate() {
        zip.add(&format!("ppt/fonts/font{}.fntdata", order + 1), &face.data);
        presentation_rels.push((
            format!("rId{}", 500 + order),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/font",
            format!("fonts/font{}.fntdata", order + 1),
        ));
    }
    let mut font_list = String::new();
    let mut typefaces: Vec<&str> = fonts.iter().map(|face| face.typeface).collect();
    typefaces.dedup();
    for typeface in typefaces {
        let slot = |bold: bool| {
            fonts
                .iter()
                .position(|face| face.typeface == typeface && face.bold == bold)
        };
        font_list.push_str(&format!(
            r#"<p:embeddedFont><p:font typeface="{}" pitchFamily="34" charset="-128"/>"#,
            escape(typeface)
        ));
        if let Some(order) = slot(false) {
            font_list.push_str(&format!(r#"<p:regular r:id="rId{}"/>"#, 500 + order));
        }
        if let Some(order) = slot(true) {
            font_list.push_str(&format!(r#"<p:bold r:id="rId{}"/>"#, 500 + order));
        }
        font_list.push_str("</p:embeddedFont>");
    }
    let (embed_attributes, font_list) = if fonts.is_empty() {
        ("", String::new())
    } else {
        (
            r#" embedTrueTypeFonts="1" saveSubsetFonts="1""#,
            format!("<p:embeddedFontLst>{font_list}</p:embeddedFontLst>"),
        )
    };
    let notes_master = if any_notes {
        r#"<p:notesMasterIdLst><p:notesMasterId r:id="rId3"/></p:notesMasterIdLst>"#
    } else {
        ""
    };
    zip.add(
        "ppt/presentation.xml",
        format!(r#"{HEAD}<p:presentation {NS}{embed_attributes}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>{notes_master}<p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/>{font_list}</p:presentation>"#).as_bytes(),
    );
    zip.add(
        "ppt/_rels/presentation.xml.rels",
        rels(&presentation_rels).as_bytes(),
    );

    // 版下とテーマ
    zip.add(
        "ppt/slideMasters/slideMaster1.xml",
        format!(r#"{HEAD}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>{TREE_HEAD}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#).as_bytes(),
    );
    zip.add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(&[
            (
                "rId1".into(),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
                "../slideLayouts/slideLayout1.xml".into(),
            ),
            (
                "rId2".into(),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
                "../theme/theme1.xml".into(),
            ),
        ])
        .as_bytes(),
    );
    zip.add(
        "ppt/slideLayouts/slideLayout1.xml",
        format!(r#"{HEAD}<p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{TREE_HEAD}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#).as_bytes(),
    );
    zip.add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[(
            "rId1".into(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
            "../slideMasters/slideMaster1.xml".into(),
        )])
        .as_bytes(),
    );
    zip.add("ppt/theme/theme1.xml", theme().as_bytes());
    if any_notes {
        zip.add("ppt/theme/theme2.xml", theme().as_bytes());
        zip.add(
            "ppt/notesMasters/notesMaster1.xml",
            format!(r#"{HEAD}<p:notesMaster {NS}><p:cSld><p:spTree>{TREE_HEAD}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:notesMaster>"#).as_bytes(),
        );
        zip.add(
            "ppt/notesMasters/_rels/notesMaster1.xml.rels",
            rels(&[(
                "rId1".into(),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
                "../theme/theme2.xml".into(),
            )])
            .as_bytes(),
        );
    }

    // スライド
    for (index, slide) in slides.iter().enumerate() {
        let n = index + 1;
        let background = slide.background.map_or(String::new(), |color| {
            let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!(
                r#"<p:bg><p:bgPr><a:solidFill><a:srgbClr val="{:02X}{:02X}{:02X}"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>"#,
                byte(color[0]),
                byte(color[1]),
                byte(color[2])
            )
        });
        zip.add(
            &format!("ppt/slides/slide{n}.xml"),
            format!(
                r#"{HEAD}<p:sld {NS}><p:cSld>{background}<p:spTree>{TREE_HEAD}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
                slide.shapes
            )
            .as_bytes(),
        );
        let mut slide_rels = vec![(
            "rId1".to_owned(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            "../slideLayouts/slideLayout1.xml".to_owned(),
        )];
        // 画像は rId10 から、リンクは rId1000 から（`pptx_slide` の決まり）
        for (order, png) in slide.media.iter().enumerate() {
            let name = format!("image{n}_{}.png", order + 1);
            zip.add(&format!("ppt/media/{name}"), png);
            slide_rels.push((
                format!("rId{}", 10 + order),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                format!("../media/{name}"),
            ));
        }
        for (order, url) in slide.links.iter().enumerate() {
            slide_rels.push((
                format!("rId{}", 1000 + order),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink",
                url.clone(),
            ));
        }
        if !slide.notes.is_empty() {
            slide_rels.push((
                "rId3".into(),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide",
                format!("../notesSlides/notesSlide{n}.xml"),
            ));
            let paragraphs: String = slide
                .notes
                .iter()
                .flat_map(|note| note.lines())
                .map(|line| {
                    format!(
                        r#"<a:p><a:r><a:rPr lang="ja-JP"/><a:t>{}</a:t></a:r></a:p>"#,
                        escape(line)
                    )
                })
                .collect();
            zip.add(
                &format!("ppt/notesSlides/notesSlide{n}.xml"),
                format!(r#"{HEAD}<p:notes {NS}><p:cSld><p:spTree>{TREE_HEAD}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"#).as_bytes(),
            );
            zip.add(
                &format!("ppt/notesSlides/_rels/notesSlide{n}.xml.rels"),
                rels(&[
                    ("rId1".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster", "../notesMasters/notesMaster1.xml".into()),
                    ("rId2".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide", format!("../slides/slide{n}.xml")),
                ])
                .as_bytes(),
            );
        }
        zip.add(
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            rels(&slide_rels).as_bytes(),
        );
    }
    zip.finish()
}

/// テーマ（色・書体・線と塗りの決まり。PowerPoint が開くのに要る最小限）
fn theme() -> String {
    let colors = [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "44546A"),
        ("lt2", "E7E6E6"),
        ("accent1", "4472C4"),
        ("accent2", "ED7D31"),
        ("accent3", "A5A5A5"),
        ("accent4", "FFC000"),
        ("accent5", "5B9BD5"),
        ("accent6", "70AD47"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ];
    let scheme: String = colors
        .iter()
        .map(|(name, hex)| format!(r#"<a:{name}><a:srgbClr val="{hex}"/></a:{name}>"#))
        .collect();
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let line = r#"<a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    let effect = r#"<a:effectStyle><a:effectLst/></a:effectStyle>"#;
    format!(
        r#"{HEAD}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="mdview"><a:themeElements><a:clrScheme name="mdview">{scheme}</a:clrScheme><a:fontScheme name="mdview"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="mdview"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst>{effect}{effect}{effect}</a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_lists_every_slide() {
        let slides = vec![
            SlideParts {
                media: vec![vec![1, 2, 3]],
                links: vec!["https://example.com/?a=1&b=2".to_owned()],
                notes: vec!["話すこと".to_owned()],
                background: Some([1.0, 1.0, 1.0, 1.0]),
                ..SlideParts::default()
            },
            SlideParts::default(),
        ];
        let fonts = [EmbeddedFace {
            typeface: "IBM Plex Sans JP",
            bold: true,
            data: vec![9, 9],
        }];
        let bytes = build(&slides, 1280.0, 720.0, "題 & 副題", &fonts);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("ppt/slides/slide2.xml"));
        assert!(text.contains("ppt/media/image1_1.png"));
        assert!(text.contains(r#"Target="https://example.com/?a=1&amp;b=2" TargetMode="External""#));
        assert!(text.contains(r#"<a:srgbClr val="FFFFFF"/></a:solidFill><a:effectLst/>"#));
        assert!(text.contains("ppt/fonts/font1.fntdata"));
        assert!(text.contains(r#"embedTrueTypeFonts="1""#));
        assert!(text.contains(r#"<p:embeddedFont><p:font typeface="IBM Plex Sans JP" pitchFamily="34" charset="-128"/><p:bold r:id="rId500"/></p:embeddedFont>"#));
        assert!(text.contains(r#"Extension="fntdata""#));
        assert!(text.contains(r#"<p:sldSz cx="12192000" cy="6858000"/>"#));
        assert!(text.contains("notesSlide1.xml") && !text.contains("notesSlide2.xml"));
        assert!(text.contains("題 &amp; 副題"));
    }
}
