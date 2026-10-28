//! Thai field values end to end (docs/thai-forms-spec.md area A): filled, saved, reopened, then
//! rendered and read back as a viewer does.

use std::sync::Arc;

use pdfcraft_cos::{Document, Object, SaveOptions, write_incremental};
use pdfcraft_forms::{FieldProps, FieldValue, NewField, acroform, add_field, appearance, fields, flags, set_props, set_value};

/// An empty document with one 600 × 800 page.
fn one_page() -> Document {
    let mut doc = Document::new_empty();
    let pages = doc.get(doc.root().unwrap()).as_dict().unwrap().reference(b"Pages").unwrap();
    let mut p = pdfcraft_cos::Dict::new();
    p.set(b"Type".to_vec(), Object::name("Page"));
    p.set(b"Parent".to_vec(), Object::Ref(pages));
    p.set(b"MediaBox".to_vec(), Object::Array(vec![0.into(), 0.into(), 600.into(), 800.into()]));
    let r = doc.add(p);
    doc.update_dict(pages, |d| {
        d.set(b"Kids".to_vec(), Object::Array(vec![Object::Ref(r)]));
        d.set(b"Count".to_vec(), Object::Int(1));
    })
    .unwrap();
    doc
}

/// `doc` saved and opened again.
fn reopen(doc: &Document) -> Document {
    Document::open(saved(doc)).expect("reopens")
}

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

/// The saved bytes of `doc`.
fn saved(doc: &Document) -> Arc<Vec<u8>> {
    Arc::new(write_incremental(doc, &SaveOptions::default()).unwrap())
}

/// Page 1's text layer (field appearances included), as the viewer extracts it.
fn page_text(doc: &Document) -> String {
    let mut r = pdfcraft_render::PageRenderer::new(saved(doc), Default::default());
    let page = r.render(pdfcraft_render::RenderRequest { page: 0, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() });
    page.text.unwrap().plain_text()
}

/// Page 1 at 2 px/pt, cropped to `rect` (user space): one darkness value (0–255) per pixel.
fn ink(doc: &Document, rect: [f64; 4]) -> Vec<u8> {
    let mut r = pdfcraft_render::PageRenderer::new(saved(doc), Default::default());
    let page = r.render(pdfcraft_render::RenderRequest { page: 0, scale: 2.0, ..Default::default() });
    assert!(page.error.is_none(), "{:?}", page.error);
    let (w, h) = (page.width as usize, page.height as usize);
    let mut out = Vec::new();
    for y in ((h as f64 - rect[3] * 2.0) as usize)..((h as f64 - rect[1] * 2.0) as usize) {
        for x in ((rect[0] * 2.0) as usize)..((rect[2] * 2.0) as usize).min(w) {
            let i = (y * w + x) * 4;
            out.push(255 - page.rgba[i..i + 3].iter().map(|&v| v as u32).sum::<u32>().div_euclid(3) as u8);
        }
    }
    out
}

/// The font dictionary `/DR` names `resource`.
fn dr_font_dict(doc: &Document, resource: &str) -> pdfcraft_cos::Dict {
    let af = acroform(doc).unwrap();
    let dr = doc.dict(af.get(b"DR").unwrap()).unwrap();
    let fonts = doc.dict(dr.get(b"Font").unwrap()).unwrap();
    doc.dict(fonts.get(resource.as_bytes()).unwrap()).unwrap()
}

#[test]
fn thai_text_is_drawn_in_sarabun() {
    if sarabun("thai_text_is_drawn_in_sarabun").is_none() {
        return;
    }
    let rect = [72.0, 700.0, 372.0, 724.0];
    let fill = |value: &str| {
        let mut doc = one_page();
        let name = add_field(&mut doc, 0, rect, &NewField::Text { multiline: false }, None).unwrap();
        set_value(&mut doc, &name, &FieldValue::Text(value.into())).unwrap();
        (reopen(&doc), name)
    };
    let text = "ที่ไม่ กำ ญุ";
    let (doc, name) = fill(text);
    let f = fields(&doc).into_iter().find(|f| f.name == name).unwrap();
    assert_eq!(f.value, [text]);
    // /DA names the embedded Sarabun; size (a new field's Auto, 0) and colour are kept.
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    let da = appearance::parse_da(&wd.get(b"DA").and_then(|o| doc.resolve(o).as_string().map(|s| s.to_text())).unwrap_or(f.da.clone()));
    assert_eq!((da.size, da.color.as_str()), (0.0, "0 g"));
    let font = dr_font_dict(&doc, &da.font);
    assert_eq!((font.name(b"Subtype"), font.name(b"BaseFont")), (Some(b"Type0".as_slice()), Some(b"Sarabun-Regular".as_slice())));
    // Viewers extract the Thai (สระอำ comes back through /ActualText, which this extractor skips).
    let extracted = page_text(&doc);
    assert!(extracted.contains("ที่ไม่") && extracted.contains("ญุ"), "{extracted:?}");
    // It is drawn, and not as the question marks of the WinAnsi fallback.
    let thai = ink(&doc, rect);
    let marks = ink(&fill(&"?".repeat(text.chars().count())).0, rect);
    assert!(thai.iter().filter(|&&v| v > 128).count() > 200);
    let differing = thai.iter().zip(&marks).filter(|(a, b)| a.abs_diff(**b) > 128).count();
    assert!(differing > 200, "{differing} pixels differ from the ? fallback");
}

/// The BaseFont of the font the normal appearance of `name`'s first widget draws with.
fn appearance_font(doc: &Document, name: &str) -> String {
    let f = fields(doc).into_iter().find(|f| f.name == name).unwrap();
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    let ap = wd.get(b"AP").and_then(|a| doc.dict(a)).unwrap();
    let Object::Stream(s) = &*doc.resolve(ap.get(b"N").unwrap()) else { panic!("normal appearance is a stream") };
    let fonts = s.dict.get(b"Resources").and_then(|r| doc.dict(r)).and_then(|r| r.get(b"Font").cloned()).and_then(|f| doc.dict(&f)).unwrap();
    let font = fonts.iter().find_map(|(_, o)| doc.dict(o)).unwrap();
    String::from_utf8_lossy(font.name(b"BaseFont").unwrap()).into_owned()
}

/// Page 1's text, one string per line, as the viewer extracts it.
fn page_lines(doc: &Document) -> Vec<String> {
    let mut r = pdfcraft_render::PageRenderer::new(saved(doc), Default::default());
    let page = r.render(pdfcraft_render::RenderRequest { page: 0, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() });
    let text = page.text.unwrap();
    let mut lines: Vec<String> = Vec::new();
    for (g, &l) in text.glyphs.iter().zip(&text.line_of) {
        let l = l as usize;
        if lines.len() <= l {
            lines.resize(l + 1, String::new());
        }
        lines[l].push_str(&g.text);
    }
    lines.into_iter().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect()
}

#[test]
fn thai_wraps_between_words() {
    if sarabun("thai_wraps_between_words").is_none() {
        return;
    }
    let words = ["ฉัน", "รัก", "ภาษา", "ไทย", "มาก"];
    let mut doc = one_page();
    let name = add_field(&mut doc, 0, [72.0, 600.0, 132.0, 724.0], &NewField::Text { multiline: true }, None).unwrap();
    set_value(&mut doc, &name, &FieldValue::Text(words.concat())).unwrap();
    let lines = page_lines(&reopen(&doc));
    assert!(lines.len() >= 2, "{lines:?}");
    // Every line is whole words, and together they are the sentence.
    assert_eq!(lines.concat(), words.concat());
    let mut rest = words.as_slice();
    for line in &lines {
        let mut taken = String::new();
        while taken.len() < line.len() && !rest.is_empty() {
            taken.push_str(rest[0]);
            rest = &rest[1..];
        }
        assert_eq!(&taken, line, "a line breaks inside a word: {lines:?}");
    }
}

#[test]
fn comb_cells_hold_whole_thai_clusters() {
    if sarabun("comb_cells_hold_whole_thai_clusters").is_none() {
        return;
    }
    let mut doc = one_page();
    let rect = [72.0, 700.0, 162.0, 724.0];
    let name = add_field(&mut doc, 0, rect, &NewField::Text { multiline: false }, None).unwrap();
    set_props(&mut doc, &name, &FieldProps { max_len: Some(Some(3)), flags: vec![(flags::COMB, true)], ..Default::default() }).unwrap();
    // MaxLen counts clusters: ที่|ดิ|น is 3, ที่|ดิ|น|ดี is 4.
    assert!(set_value(&mut doc, &name, &FieldValue::Text("ที่ดินดี".into())).is_err());
    set_value(&mut doc, &name, &FieldValue::Text("ที่ดิน".into())).unwrap();
    let doc = reopen(&doc);
    let mut r = pdfcraft_render::PageRenderer::new(saved(&doc), Default::default());
    let page = r.render(pdfcraft_render::RenderRequest { page: 0, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() });
    let text = page.text.unwrap();
    let cell = (rect[2] - rect[0]) / 3.0;
    for (i, base) in ["ท", "ด", "น"].iter().enumerate() {
        let g = text.glyphs.iter().find(|g| g.text == *base).unwrap();
        let centre = f64::from(g.rect[0] + g.rect[2]) / 2.0;
        let (lo, hi) = (rect[0] + cell * i as f64, rect[0] + cell * (i + 1) as f64);
        assert!(centre > lo && centre < hi, "{base} at {centre}, cell {i} is {lo}..{hi}");
    }
}

#[test]
fn thai_choices_are_drawn_in_sarabun() {
    if sarabun("thai_choices_are_drawn_in_sarabun").is_none() {
        return;
    }
    let mut doc = one_page();
    let options = vec!["กรุงเทพฯ".to_owned(), "เชียงใหม่".to_owned()];
    let combo = add_field(&mut doc, 0, [72.0, 700.0, 272.0, 724.0], &NewField::Combo { options: options.clone(), editable: false }, None).unwrap();
    let list = add_field(&mut doc, 0, [72.0, 560.0, 272.0, 660.0], &NewField::List { options, multi: false }, None).unwrap();
    set_value(&mut doc, &combo, &FieldValue::Choice(vec!["กรุงเทพฯ".into()])).unwrap();
    set_value(&mut doc, &list, &FieldValue::Choice(vec!["เชียงใหม่".into()])).unwrap();
    let doc = reopen(&doc);
    assert_eq!(appearance_font(&doc, &combo), "Sarabun-Regular");
    assert_eq!(appearance_font(&doc, &list), "Sarabun-Regular");
    let lines = page_lines(&doc);
    // The combo shows its value; the list shows every option.
    assert_eq!(lines.iter().filter(|l| l.as_str() == "กรุงเทพฯ").count(), 2, "{lines:?}");
    assert!(lines.iter().any(|l| l == "เชียงใหม่"), "{lines:?}");
}
