//! Thai field values (docs/thai-forms-spec.md area A): drawn in the Thai document face.

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

/// The font dictionary `/DR` names `resource`.
fn dr_font_dict(doc: &Document, resource: &str) -> pdfcraft_cos::Dict {
    let af = acroform(doc).unwrap();
    let dr = doc.dict(af.get(b"DR").unwrap()).unwrap();
    let fonts = doc.dict(dr.get(b"Font").unwrap()).unwrap();
    doc.dict(fonts.get(resource.as_bytes()).unwrap()).unwrap()
}

/// A text field whose `/DA` is `da`, filled with `value`; returns the BaseFont `/DA` now names.
fn thai_base_font_for(da: &str, value: &str) -> String {
    let mut doc = one_page();
    let name = add_field(&mut doc, 0, [72.0, 700.0, 372.0, 724.0], &NewField::Text { multiline: false }, None).unwrap();
    let f = fields(&doc).into_iter().find(|f| f.name == name).unwrap();
    doc.update_dict(f.widgets[0].obj, |d| d.set(b"DA".to_vec(), pdfcraft_cos::PdfString::text(da))).unwrap();
    set_value(&mut doc, &name, &FieldValue::Text(value.into())).unwrap();
    let doc = reopen(&doc);
    let f = fields(&doc).into_iter().find(|f| f.name == name).unwrap();
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    let da = appearance::parse_da(&wd.get(b"DA").and_then(|o| doc.resolve(o).as_string().map(|s| s.to_text())).unwrap());
    String::from_utf8_lossy(dr_font_dict(&doc, &da.font).name(b"BaseFont").unwrap()).into_owned()
}

#[test]
fn bold_and_italic_fields_use_the_matching_sarabun() {
    let Some(faces) = sarabun("bold_and_italic_fields_use_the_matching_sarabun") else { return };
    assert_eq!(thai_base_font_for("/HeBo 12 Tf 0 g", "ตัวหนา"), "Sarabun-Bold");
    assert_eq!(thai_base_font_for("/TiBI 12 Tf 0 g", "ตัวหนาเอียง"), "Sarabun-BoldItalic");
    assert_eq!(thai_base_font_for("/HeOb 12 Tf 0 g", "ตัวเอียง"), "Sarabun-Italic");
    assert_eq!(thai_base_font_for("/Helv 12 Tf 0 g", "ธรรมดา"), "Sarabun-Regular");
    // A style that wasn't found falls back to Regular (never a faked bold).
    let regular_only = pdfcraft_fonts::thai::ThaiFaces { bold: None, italic: None, bold_italic: None, ..faces };
    let mut doc = one_page();
    let name = add_field(&mut doc, 0, [72.0, 700.0, 372.0, 724.0], &NewField::Text { multiline: false }, None).unwrap();
    let f = fields(&doc).into_iter().find(|f| f.name == name).unwrap();
    doc.update_dict(f.widgets[0].obj, |d| d.set(b"DA".to_vec(), pdfcraft_cos::PdfString::text("/HeBo 12 Tf 0 g"))).unwrap();
    let f = fields(&doc).into_iter().find(|f| f.name == name).unwrap();
    let (stream, _) = appearance::field_appearance_written_with(&mut doc, &f, &f.widgets[0], &["ตัวหนา".into()], true, Some(&regular_only));
    let fonts = stream.dict.get(b"Resources").and_then(|r| doc.dict(r)).and_then(|r| r.get(b"Font").cloned()).and_then(|f| doc.dict(&f)).unwrap();
    let font = fonts.iter().find_map(|(_, o)| doc.dict(o)).unwrap();
    assert_eq!(font.name(b"BaseFont"), Some(b"Sarabun-Regular".as_slice()));
}

/// How many embedded Sarabun files `doc` holds.
fn sarabun_files(doc: &Document) -> usize {
    doc.object_numbers()
        .into_iter()
        .filter(|&num| {
            let o = doc.try_get(num);
            matches!(o.as_deref(), Ok(Object::Dict(d)) if d.name(b"Type") == Some(b"FontDescriptor") && d.contains(b"FontFile2")
                && d.name(b"FontName").is_some_and(|n| n.starts_with(b"Sarabun")))
        })
        .count()
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

#[test]
fn sarabun_is_embedded_once_and_kept_for_the_field() {
    if sarabun("sarabun_is_embedded_once_and_kept_for_the_field").is_none() {
        return;
    }
    let mut doc = one_page();
    let a = add_field(&mut doc, 0, [72.0, 700.0, 372.0, 724.0], &NewField::Text { multiline: false }, None).unwrap();
    let b = add_field(&mut doc, 0, [72.0, 660.0, 372.0, 684.0], &NewField::Text { multiline: false }, None).unwrap();
    set_value(&mut doc, &a, &FieldValue::Text("ชื่อ".into())).unwrap();
    set_value(&mut doc, &b, &FieldValue::Text("นามสกุล".into())).unwrap();
    let mut doc = reopen(&doc);
    set_value(&mut doc, &a, &FieldValue::Text("ชื่อใหม่".into())).unwrap();
    // Retyped in Latin, the field stays in Sarabun (its /DA names it).
    set_value(&mut doc, &b, &FieldValue::Text("Smith".into())).unwrap();
    let doc = reopen(&doc);
    assert_eq!(sarabun_files(&doc), 1);
    assert_eq!(appearance_font(&doc, &b), "Sarabun-Regular");
}

#[test]
fn without_a_thai_face_the_value_is_kept_and_drawn_in_helvetica() {
    let mut doc = one_page();
    let name = add_field(&mut doc, 0, [72.0, 700.0, 372.0, 724.0], &NewField::Text { multiline: false }, None).unwrap();
    let f = fields(&doc).into_iter().find(|f| f.name == name).unwrap();
    let (stream, undrawn) = appearance::field_appearance_written_with(&mut doc, &f, &f.widgets[0], &["ภาษาไทย".into()], true, None);
    assert_eq!(undrawn, Some('ภ'));
    let fonts = stream.dict.get(b"Resources").and_then(|r| doc.dict(r)).and_then(|r| r.get(b"Font").cloned()).and_then(|f| doc.dict(&f)).unwrap();
    let font = fonts.iter().find_map(|(_, o)| doc.dict(o)).unwrap();
    assert_eq!(font.name(b"BaseFont"), Some(b"Helvetica".as_slice()));
    assert_eq!(sarabun_files(&doc), 0);
    // The /DA is left alone.
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    assert_eq!(wd.get(b"DA").and_then(|o| o.as_string()).map(|s| s.to_text()).as_deref(), Some("/Helv 0 Tf 0 g"));
}

/// The font size (`Tf`) and the first baseline (`Tm`'s y) of `name`'s normal appearance.
fn size_and_baseline(doc: &Document, name: &str) -> (f64, f64) {
    let f = fields(doc).into_iter().find(|f| f.name == name).unwrap();
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    let ap = wd.get(b"AP").and_then(|a| doc.dict(a)).unwrap();
    let Object::Stream(s) = &*doc.resolve(ap.get(b"N").unwrap()) else { panic!("normal appearance is a stream") };
    let ops = String::from_utf8_lossy(&s.decoded().unwrap()).into_owned();
    let toks: Vec<&str> = ops.split_whitespace().collect();
    let at = |op: &str, back: usize| toks.iter().position(|t| *t == op).map(|i| toks[i - back].parse::<f64>().unwrap()).unwrap();
    (at("Tf", 1), at("Tm", 1))
}

#[test]
fn auto_sized_thai_fits_the_field_with_a_steady_baseline() {
    let Some(faces) = sarabun("auto_sized_thai_fits_the_field_with_a_steady_baseline") else { return };
    let (ascent, descent) = pdfcraft_fonts::thai::vertical_metrics(&faces.regular).unwrap();
    let rect = [72.0, 700.0, 372.0, 716.0];
    let fill = |value: &str| {
        let mut doc = one_page();
        let name = add_field(&mut doc, 0, rect, &NewField::Text { multiline: false }, None).unwrap();
        set_props(&mut doc, &name, &FieldProps { font_size: Some(0.0), ..Default::default() }).unwrap();
        set_value(&mut doc, &name, &FieldValue::Text(value.into())).unwrap();
        size_and_baseline(&reopen(&doc), &name)
    };
    let (size, y) = fill("ที่");
    let height = rect[3] - rect[1];
    assert!(y + ascent * size <= height - 1.0 + 1e-6 && y + descent * size >= 1.0 - 1e-6, "size {size} at {y} overflows {height}");
    assert_eq!(fill("กา"), (size, y));
}

#[test]
fn sarabun_matches_the_visual_size_of_the_field_font() {
    let Some(faces) = sarabun("sarabun_matches_the_visual_size_of_the_field_font") else { return };
    // A Thai form's field font (BrowalliaUPC's declared metrics): small letters for its size.
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Annots [6 0 R] >>".into(),
        "<< /Fields [6 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /BrowalliaUPC 5 0 R >> >> >>".into(),
        "<< /Type /Font /Subtype /TrueType /BaseFont /BrowalliaUPC /FirstChar 32 /LastChar 32 /Widths [250] /Encoding /WinAnsiEncoding /FontDescriptor 7 0 R >>".into(),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (attention) /Ff 4096 /Rect [115 624 280 642] /P 3 0 R /DA (/BrowalliaUPC 12 Tf 0 g) >>".into(),
        "<< /Type /FontDescriptor /FontName /BrowalliaUPC /Flags 32 /FontBBox [-459 -335 791 921] /ItalicAngle 0 /Ascent 921 /Descent -335 /CapHeight 467 /XHeight 338 /StemV 56 >>".into(),
    ];
    let mut doc = document(&objs);
    set_value(&mut doc, "attention", &FieldValue::Text("บจก. ซัคเซส พลาส จำกัด".into())).unwrap();
    let doc = reopen(&doc);
    let (size, _) = size_and_baseline(&doc, "attention");
    // Sarabun's lowercase height at the new size is BrowalliaUPC's at 12 pt (0.338 em).
    let x_height = pdfcraft_fonts::thai::x_height(&faces.regular).unwrap();
    assert!((size * x_height - 12.0 * 0.338).abs() < 0.05, "size {size}: x-height {} pt", size * x_height);
    // And /DA says so, for other viewers.
    let f = fields(&doc).into_iter().find(|f| f.name == "attention").unwrap();
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    let da = appearance::parse_da(&wd.get(b"DA").and_then(|o| doc.resolve(o).as_string().map(|s| s.to_text())).unwrap());
    assert_eq!((da.font.as_str(), da.size), ("Sarabun-Regular", size));
    // A field set in Sarabun keeps that size when it is filled again.
    let mut doc = doc;
    set_value(&mut doc, "attention", &FieldValue::Text("บจก. ซัคเซส พลาส จำกัด (สำนักงานใหญ่)".into())).unwrap();
    assert_eq!(size_and_baseline(&reopen(&doc), "attention").0, size);
}

/// Multiline fields side by side as a table (code, description): the Thai one, drawn in Sarabun,
/// has the same line pitch as the Latin one, so their rows line up.
#[test]
fn a_thai_multiline_field_lines_up_with_its_latin_neighbour() {
    if sarabun("a_thai_multiline_field_lines_up_with_its_latin_neighbour").is_none() {
        return;
    }
    let mut doc = one_page();
    let code = add_field(&mut doc, 0, [72.0, 500.0, 120.0, 700.0], &NewField::Text { multiline: true }, None).unwrap();
    let item = add_field(&mut doc, 0, [124.0, 500.0, 372.0, 700.0], &NewField::Text { multiline: true }, None).unwrap();
    set_value(&mut doc, &code, &FieldValue::Text("A\n1\n2\n3\n4".into())).unwrap();
    set_value(&mut doc, &item, &FieldValue::Text("ระบบ Intercom\nที่กันฝน\nญุ่ปั้น\nงานติดตั้งอุปกรณ์\nกำ".into())).unwrap();
    let doc = reopen(&doc);
    let all = fields(&doc);
    assert!(field(&all, &item).da.contains("Sarabun"), "{}", field(&all, &item).da);
    // The baseline of each line: the y operand of its text matrix.
    let baselines = |name: &str| -> Vec<String> {
        let text = ap(&doc, &field(&all, name).widgets[0]);
        let words: Vec<&str> = text.split_whitespace().collect();
        words.windows(2).filter(|w| w[1] == "Tm").map(|w| w[0].to_owned()).collect()
    };
    let (latin, thai) = (baselines(&code), baselines(&item));
    assert_eq!(latin.len(), 5, "{latin:?}");
    // The same line pitch, so the rows never drift; the Thai first line only sits low enough for
    // its tallest marks (a_thai_first_line_keeps_its_stacked_marks_inside_the_field).
    let (latin, thai): (Vec<f64>, Vec<f64>) = (latin.iter().map(|y| y.parse().unwrap()).collect(), thai.iter().map(|y| y.parse().unwrap()).collect());
    let offset = latin[0] - thai[0];
    assert!((0.0..=2.5).contains(&offset), "{offset}");
    assert!(latin.iter().zip(&thai).all(|(l, t)| (l - t - offset).abs() < 1e-3), "{latin:?} {thai:?}");
}

/// A Thai first line keeps its tallest marks inside the field (the box clips them): the tone mark
/// over ื in หมื่น, at the Auto size of a one-line box (12 pt) and at a fixed 8 pt.
#[test]
fn a_thai_first_line_keeps_its_stacked_marks_inside_the_field() {
    let Some(faces) = sarabun("a_thai_first_line_keeps_its_stacked_marks_inside_the_field") else {
        return;
    };
    let top = pdfcraft_fonts::thai::mark_top(&faces.regular).unwrap();
    assert!(top > 1.1, "ื่ reaches above the ascent: {top}");
    // The amount in words of an invoice: a multiline field one line high.
    for (size, rect) in [(0.0, [51.3, 166.6, 401.8, 194.0]), (8.0, [108.0, 400.0, 353.0, 481.5])] {
        let mut doc = one_page();
        let name = add_field(&mut doc, 0, rect, &NewField::Text { multiline: true }, None).unwrap();
        set_props(&mut doc, &name, &FieldProps { font_size: Some(size), ..FieldProps::default() }).unwrap();
        set_value(&mut doc, &name, &FieldValue::Text("ห้าหมื่นเจ็ดพันเจ็ดร้อยแปดสิบบาทถ้วน".into())).unwrap();
        let doc = reopen(&doc);
        let all = fields(&doc);
        let text = ap(&doc, &field(&all, &name).widgets[0]);
        let words: Vec<&str> = text.split_whitespace().collect();
        let drawn = words.windows(2).find(|w| w[1] == "Tf").and_then(|w| w[0].parse::<f64>().ok()).unwrap();
        let first = words.windows(2).find(|w| w[1] == "Tm").and_then(|w| w[0].parse::<f64>().ok()).unwrap();
        let height = rect[3] - rect[1];
        assert!(first + top * drawn <= height + 1e-6, "size {drawn}: the marks reach {} above a {height} box", first + top * drawn);
    }
}
