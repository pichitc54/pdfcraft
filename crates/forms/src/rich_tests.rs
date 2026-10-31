//! Rich values of text fields (docs/thai-forms-spec.md area E): `/RV` and `/V` in step, each
//! styled run drawn in its own style, other apps' rich values read and kept.

use super::*;
use crate::rich::{Align, RichParagraph, RichValue, RunStyle, StyledRun, Vertical};

/// Sarabun installed as the process's document faces; `None` (test skipped) without it.
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

/// A text field that allows rich text formatting, on page 1 at `rect`.
fn rich_field(doc: &mut Document, rect: [f64; 4], multiline: bool, size: f64) -> String {
    let name = add_field(doc, 0, rect, &NewField::Text { multiline }, None).unwrap();
    set_props(doc, &name, &FieldProps { flags: vec![(flags::RICH_TEXT, true)], font_size: Some(size), ..Default::default() }).unwrap();
    name
}

fn run(text: &str, style: RunStyle) -> StyledRun {
    StyledRun { text: text.into(), style }
}

fn plain(text: &str) -> StyledRun {
    run(text, RunStyle::default())
}

fn bold() -> RunStyle {
    RunStyle { bold: Some(true), ..Default::default() }
}

fn para(runs: Vec<StyledRun>) -> RichParagraph {
    RichParagraph { runs, ..Default::default() }
}

fn field_named(doc: &Document, name: &str) -> Field {
    fields(doc).into_iter().find(|f| f.name == name).unwrap()
}

/// The BaseFonts of the fonts `name`'s normal appearance uses.
fn appearance_fonts(doc: &Document, name: &str) -> Vec<String> {
    let f = field_named(doc, name);
    let wd = doc.get(f.widgets[0].obj).as_dict().cloned().unwrap();
    let ap = wd.get(b"AP").and_then(|a| doc.dict(a)).unwrap();
    let Object::Stream(s) = &*doc.resolve(ap.get(b"N").unwrap()) else { panic!("normal appearance is a stream") };
    let fonts = s.dict.get(b"Resources").and_then(|r| doc.dict(r)).and_then(|r| r.get(b"Font").cloned()).and_then(|f| doc.dict(&f)).unwrap();
    let mut out: Vec<String> =
        fonts.iter().filter_map(|(_, o)| doc.dict(o)).filter_map(|d| d.name(b"BaseFont").map(|b| String::from_utf8_lossy(b).into_owned())).collect();
    out.sort();
    out
}

/// The field dictionary's `/RV` and `/DS` as text.
fn rv_and_ds(doc: &Document, name: &str) -> (Option<String>, Option<String>) {
    let d = doc.get(field_named(doc, name).obj).as_dict().cloned().unwrap();
    let text = |k: &[u8]| d.get(k).and_then(|o| doc.resolve(o).as_string().map(|s| s.to_text()));
    (text(b"RV"), text(b"DS"))
}

/// The (x, y) operands of every `Tm` in `ops`, with the `Tf` size in force.
fn placements(ops: &str) -> Vec<(f64, f64, f64)> {
    let toks: Vec<&str> = ops.split_whitespace().collect();
    let mut size = 0.0;
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if *t == "Tf" {
            size = toks[i - 1].parse().unwrap();
        }
        if *t == "Tm" {
            out.push((toks[i - 2].parse().unwrap(), toks[i - 1].parse().unwrap(), size));
        }
    }
    out
}

#[test]
fn a_rich_value_is_saved_beside_the_plain_value_and_reads_back() {
    let mut doc = one_page();
    let name = rich_field(&mut doc, [72.0, 700.0, 372.0, 724.0], false, 12.0);
    let rv = RichValue { paragraphs: vec![para(vec![plain("Name: "), run("Somchai", bold()), plain(" Jaidee")])] };
    set_value(&mut doc, &name, &FieldValue::Rich(rv.clone())).unwrap();
    let doc = reopen(&doc);
    let f = field_named(&doc, &name);
    assert_eq!(f.value, ["Name: Somchai Jaidee"]);
    assert_eq!(f.rich_value(), Some(&rv));
    let (stored, ds) = rv_and_ds(&doc, &name);
    assert!(stored.unwrap().contains("font-weight:bold"));
    assert_eq!(ds.as_deref(), Some("font: Helvetica 12pt; color:#000000"));
    // The bold word is drawn in Helvetica-Bold, the rest in the field's Helvetica.
    let fonts = appearance_fonts(&doc, &name);
    assert!(fonts.contains(&"Helvetica-Bold".to_string()) && fonts.contains(&"Helvetica".to_string()), "{fonts:?}");
}

#[test]
fn a_plain_value_or_an_ordinary_field_carries_no_rich_value() {
    let mut doc = one_page();
    let rich = rich_field(&mut doc, [72.0, 700.0, 372.0, 724.0], false, 12.0);
    let ordinary = add_field(&mut doc, 0, [72.0, 600.0, 372.0, 624.0], &NewField::Text { multiline: false }, None).unwrap();
    let rv = RichValue { paragraphs: vec![para(vec![run("Bold", bold())])] };
    // An ordinary field takes only the plain text.
    set_value(&mut doc, &ordinary, &FieldValue::Rich(rv.clone())).unwrap();
    // A plain value replaces a rich one entirely (scripts, calculations, plain typing).
    set_value(&mut doc, &rich, &FieldValue::Rich(rv)).unwrap();
    set_value(&mut doc, &rich, &FieldValue::Text("Plain".into())).unwrap();
    let doc = reopen(&doc);
    assert_eq!(field_named(&doc, &ordinary).value, ["Bold"]);
    assert_eq!(rv_and_ds(&doc, &ordinary).0, None);
    assert_eq!(rv_and_ds(&doc, &rich).0, None);
    assert_eq!(field_named(&doc, &rich).rich_value(), None);
}

#[test]
fn comb_and_password_fields_ignore_formatting() {
    for flag in [flags::COMB, flags::PASSWORD] {
        let mut doc = one_page();
        let name = rich_field(&mut doc, [72.0, 700.0, 372.0, 724.0], false, 12.0);
        set_props(&mut doc, &name, &FieldProps { flags: vec![(flag, true)], max_len: Some(Some(8)), ..Default::default() }).unwrap();
        set_value(&mut doc, &name, &FieldValue::Rich(RichValue { paragraphs: vec![para(vec![run("Bold", bold())])] })).unwrap();
        let doc = reopen(&doc);
        assert_eq!(rv_and_ds(&doc, &name).0, None, "flag {flag}");
        assert_eq!(field_named(&doc, &name).value, ["Bold"]);
    }
}

#[test]
fn max_len_counts_the_clusters_of_a_rich_value() {
    let mut doc = one_page();
    let name = rich_field(&mut doc, [72.0, 700.0, 372.0, 724.0], false, 12.0);
    set_props(&mut doc, &name, &FieldProps { max_len: Some(Some(2)), ..Default::default() }).unwrap();
    let rv = |t: &str| FieldValue::Rich(RichValue { paragraphs: vec![para(vec![run(t, bold())])] });
    set_value(&mut doc, &name, &rv("ที่นี่")).unwrap();
    assert!(set_value(&mut doc, &name, &rv("ที่นี่ค")).is_err());
}

/// A one-page form whose field `rich` is a rich-text field holding `value` and `/RV` `rv`.
fn form_with_rv(value: &str, rv: &str, multiline: bool) -> Document {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
    let ff = flags::RICH_TEXT | if multiline { flags::MULTILINE } else { 0 };
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>".into(),
        "<< /Fields [5 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 6 0 R >> >> >>".into(),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (rich) /Ff {ff} /Rect [72 600 372 700] /P 3 0 R /DA (/Helv 12 Tf 0 g) /V ({}) /RV ({}) >>",
            esc(value),
            esc(rv)
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    ];
    document(&objs)
}

#[test]
fn another_apps_rich_value_is_read_drawn_and_kept() {
    let rv = r#"<?xml version="1.0"?><body xmlns="http://www.w3.org/1999/xhtml" xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/" xfa:spec="2.0.2" style="font:12pt Helvetica"><p style="text-align:center">Area <span style="color:#ff0000;text-decoration:underline">10 m</span><span style="vertical-align:super">2</span></p><p><b>Bold</b> and <i>italic</i> &amp; <span style="font-family:'Times New Roman';font-size:14pt;letter-spacing:1pt;font-stretch:80%">Times</span><span xmlns:x="urn:x" x:unknown="kept">!</span></p></body>"#;
    let doc = form_with_rv("Area 10 m2\rBold and italic & Times!", rv, true);
    let f = field_named(&doc, "rich");
    let got = f.rich_value().cloned().expect("the rich value reads");
    assert_eq!(got.paragraphs.len(), 2);
    assert_eq!(got.paragraphs[0].align, Some(Align::Center));
    let area = &got.paragraphs[0].runs;
    assert_eq!(area[1].text, "10 m");
    assert_eq!((area[1].style.color, area[1].style.underline), (Some([1.0, 0.0, 0.0]), true));
    assert_eq!((area[2].text.as_str(), area[2].style.vertical), ("2", Vertical::Super));
    let second = &got.paragraphs[1].runs;
    assert_eq!((second[0].text.as_str(), second[0].style.bold), ("Bold", Some(true)));
    assert_eq!((second[2].text.as_str(), second[2].style.italic), ("italic", Some(true)));
    let times = second.iter().find(|r| r.text == "Times").unwrap();
    assert_eq!(
        (times.style.family.as_deref(), times.style.size, times.style.char_spacing, times.style.scale),
        (Some("Times New Roman"), Some(14.0), Some(1.0), Some(80.0))
    );
    // Redrawn (a restyle, say), it is drawn with all of it; the /RV is left exactly as it was.
    let mut doc = doc;
    redraw_field(&mut doc, "rich").unwrap();
    let doc = reopen(&doc);
    assert_eq!(rv_and_ds(&doc, "rich").0.as_deref(), Some(rv));
    let fonts = appearance_fonts(&doc, "rich");
    for want in ["Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Times-Roman"] {
        assert!(fonts.contains(&want.to_string()), "{want} in {fonts:?}");
    }
    let f = field_named(&doc, "rich");
    let ops = ap(&doc, &f.widgets[0]);
    assert!(ops.contains("1 0 0 rg"), "the red run: {ops}");
    assert!(ops.contains(" re f"), "the underline: {ops}");
    assert!(ops.contains("80 Tz") && ops.contains("1 Tc"), "the Times run's scale and spacing: {ops}");
    // The superscript 2 sits above the line, smaller.
    let at = placements(&ops);
    let (_, base_y, base_size) = at[0];
    let sup = at.iter().find(|(_, y, s)| *y > base_y + 1.0 && *s < base_size).copied();
    assert!(sup.is_some(), "a raised, smaller run: {at:?}");
    // The centred first line starts right of the left padding.
    assert!(at[0].0 > 40.0, "centred: {at:?}");
}

#[test]
fn a_rich_value_that_no_longer_shows_the_value_or_does_not_read_is_ignored() {
    // Out of step with /V (another app changed /V only): the plain value is drawn.
    let doc = form_with_rv("New text", "<body><p><b>Old text</b></p></body>", false);
    assert_eq!(field_named(&doc, "rich").rich_value(), None);
    // Unreadable markup never fails the open.
    for broken in ["<body><p><b>Text", "<<<>>>", &"<span>".repeat(200), ""] {
        let mut doc = form_with_rv("Text", broken, false);
        let f = field_named(&doc, "rich");
        assert_eq!(f.value, ["Text"]);
        redraw_field(&mut doc, "rich").unwrap();
    }
}

#[test]
fn a_line_is_as_tall_as_its_largest_run_and_sized_runs_keep_their_size() {
    let mut doc = one_page();
    let name = rich_field(&mut doc, [72.0, 500.0, 372.0, 700.0], true, 0.0);
    let big = RunStyle { size: Some(20.0), ..Default::default() };
    let rv = RichValue { paragraphs: vec![para(vec![plain("small")]), para(vec![run("BIG", big), plain(" small")]), para(vec![plain("small")])] };
    set_value(&mut doc, &name, &FieldValue::Rich(rv)).unwrap();
    let doc = reopen(&doc);
    let ops = ap(&doc, &field_named(&doc, &name).widgets[0]);
    let at = placements(&ops);
    // Auto size: the unsized runs share one size; the 20 pt run keeps 20.
    let small = at[0].2;
    assert!(at.iter().any(|(_, _, s)| *s == 20.0), "{at:?}");
    assert!(at.iter().filter(|(_, _, s)| *s != 20.0).all(|(_, _, s)| *s == small), "{at:?}");
    let ys: Vec<f64> = at.iter().map(|(_, y, _)| *y).fold(Vec::new(), |mut v, y| {
        if v.last() != Some(&y) {
            v.push(y);
        }
        v
    });
    assert_eq!(ys.len(), 3, "{at:?}");
    assert!((ys[0] - ys[1] - 20.0 * 1.15).abs() < 0.01, "the big line: {ys:?}");
    assert!((ys[1] - ys[2] - small * 1.15).abs() < 0.01, "the next line: {ys:?}");
}

#[test]
fn thai_runs_are_drawn_in_sarabun_in_their_style() {
    if sarabun("thai_runs_are_drawn_in_sarabun_in_their_style").is_none() {
        return;
    }
    let mut doc = one_page();
    let name = rich_field(&mut doc, [72.0, 700.0, 372.0, 724.0], false, 12.0);
    let rv = RichValue { paragraphs: vec![para(vec![plain("ชื่อ: "), run("สมชาย", bold()), plain(" ใจดี")])] };
    set_value(&mut doc, &name, &FieldValue::Rich(rv.clone())).unwrap();
    let doc = reopen(&doc);
    assert_eq!(field_named(&doc, &name).rich_value(), Some(&rv));
    assert_eq!(appearance_fonts(&doc, &name), ["Sarabun-Bold", "Sarabun-Regular"]);
    let ops = ap(&doc, &field_named(&doc, &name).widgets[0]);
    assert!(!ops.contains("(?"), "nothing drawn as ?: {ops}");
}

#[test]
fn thai_inside_a_latin_family_goes_to_sarabun_and_the_latin_stays() {
    if sarabun("thai_inside_a_latin_family_goes_to_sarabun_and_the_latin_stays").is_none() {
        return;
    }
    let mut doc = one_page();
    let name = rich_field(&mut doc, [72.0, 700.0, 372.0, 724.0], false, 12.0);
    let times = RunStyle { family: Some("Times".into()), italic: Some(true), ..Default::default() };
    let rv = RichValue { paragraphs: vec![para(vec![plain("บริษัท "), run("Acme ไทย", times)])] };
    set_value(&mut doc, &name, &FieldValue::Rich(rv)).unwrap();
    let doc = reopen(&doc);
    // The Latin in Times Italic, the Thai beside it in Sarabun Italic, the unstyled Thai in Sarabun.
    assert_eq!(appearance_fonts(&doc, &name), ["Sarabun-Italic", "Sarabun-Regular", "Times-Italic"]);
}

#[test]
fn a_thai_first_line_of_a_rich_value_keeps_its_stacked_marks_inside() {
    let Some(faces) = sarabun("a_thai_first_line_of_a_rich_value_keeps_its_stacked_marks_inside") else { return };
    let top = pdfcraft_fonts::thai::mark_top(faces.for_style(true, false)).unwrap();
    let mut doc = one_page();
    let rect = [51.3, 166.6, 401.8, 194.0];
    let name = rich_field(&mut doc, rect, true, 12.0);
    set_value(&mut doc, &name, &FieldValue::Rich(RichValue { paragraphs: vec![para(vec![run("ห้าหมื่นบาทถ้วน", bold())])] })).unwrap();
    let doc = reopen(&doc);
    let at = placements(&ap(&doc, &field_named(&doc, &name).widgets[0]));
    let (_, y, size) = at[0];
    assert!(y + top * size <= rect[3] - rect[1] + 1e-6, "marks reach {} in a {} box", y + top * size, rect[3] - rect[1]);
}
