//! Thai comment text (docs/thai-forms-spec.md area C): drawn in the Thai document face.

use super::*;

/// Sarabun from `PDFCRAFT_THAI_FONT_DIR` or craft-fonts, installed as the process's document
/// faces; `None` (and the test is skipped) without it.
fn sarabun(test: &str) -> Option<pdfcraft_fonts::thai::ThaiFaces> {
    use pdfcraft_fonts::thai;
    let dirs: Vec<std::path::PathBuf> = std::env::var_os(thai::DIR_VAR).map(Into::into).into_iter().collect();
    let faces = thai::find_document_faces(&dirs);
    if faces.is_none() {
        eprintln!("skipping {test}: set {} to a folder with Sarabun (or build with craft-fonts) to run it", thai::DIR_VAR);
    }
    thai::install_document_faces(faces.clone());
    faces
}

/// The BaseFont of the font the annotation's normal appearance draws its text with.
fn ap_font(doc: &Document, d: &Dict) -> String {
    let ap = doc.resolve(d.get(b"AP").unwrap());
    let n = doc.resolve(ap.as_dict().unwrap().get(b"N").unwrap());
    let Object::Stream(s) = &*n else { panic!("normal appearance is a stream") };
    let res = doc.resolve(s.dict.get(b"Resources").unwrap());
    let fonts = doc.resolve(res.as_dict().unwrap().get(b"Font").unwrap());
    let font = fonts.as_dict().unwrap().iter().find_map(|(_, o)| doc.resolve(o).as_dict().cloned()).unwrap();
    String::from_utf8_lossy(font.name(b"BaseFont").unwrap()).into_owned()
}

/// How many embedded Sarabun font files `doc` holds.
fn sarabun_files(doc: &Document) -> usize {
    doc.object_numbers()
        .into_iter()
        .filter(|&num| {
            matches!(doc.try_get(num).as_deref(), Ok(Object::Dict(d)) if d.name(b"Type") == Some(b"FontDescriptor") && d.contains(b"FontFile2")
                && d.name(b"FontName").is_some_and(|n| n.starts_with(b"Sarabun")))
        })
        .count()
}

#[test]
fn thai_comment_text_is_drawn_in_sarabun() {
    if sarabun("thai_comment_text_is_drawn_in_sarabun").is_none() {
        return;
    }
    let mut doc = fixture();
    let typed = Shape::Typewriter { rect: [100.0, 700.0, 300.0, 716.0], font_size: 11.0 };
    let f = add_annotation(
        &mut doc,
        &NewAnnotation {
            page: 0, style: Style::default_for(&typed), shape: typed, contents: "ขอแจ้งหนี้ตามรายการ".into(), author: "Ada".into()
        },
        &meta("x"),
    )
    .unwrap();
    let mut boxed = new(0, Shape::TextBox { rect: [100.0, 600.0, 300.0, 650.0], font_size: 12.0 });
    boxed.contents = "ที่ไม่ กำ ญุ ป่า".into();
    let t = add_annotation(&mut doc, &boxed, &meta("t")).unwrap();
    let mut doc = reopen(&doc);
    // A restyle redraws it in Sarabun again instead of refusing.
    set_style(&mut doc, 0, t, StyleChange::new(Some([1.0, 0.0, 0.0]), None, None, None), &meta("")).unwrap();
    let doc = reopen(&doc);
    for i in [f, t] {
        let d = &list(&doc, 0)[i];
        assert_eq!(pdfcraft_fonts::undrawn_mark(d), None);
        assert_eq!(ap_font(&doc, d), "Sarabun-Regular");
        assert!(!ap_content(&doc, d).contains('?'), "{}", ap_content(&doc, d));
    }
    assert_eq!(sarabun_files(&doc), 1);
    // Text Sarabun can't draw keeps the standard fonts and is still reported.
    let mut doc = doc;
    let mut chinese = new(0, Shape::TextBox { rect: [100.0, 500.0, 300.0, 550.0], font_size: 12.0 });
    chinese.contents = "签名".into();
    let c = add_annotation(&mut doc, &chinese, &meta("c")).unwrap();
    assert_eq!(pdfcraft_fonts::undrawn_mark(&list(&doc, 0)[c]), Some('签'));
}
