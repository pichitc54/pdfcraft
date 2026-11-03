//! Formatting a rich text field while filling it (docs/thai-forms-spec.md area E): ⌘B, ⌘I, ⌘U and
//! the Format text panel act on the selection or the caret, and the field commits a rich value.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_engine::field_flags;
use pdfcraft_ui_egui::PdfCraftApp;

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("form.pdf", None, include_bytes!("../data/form.pdf").to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "150").unwrap();
        app
    });
    crate::support::settle(&mut h);
    // "city" allows rich text formatting; "name" doesn't.
    let props = pdfcraft_engine::FieldProps { flags: vec![(field_flags::RICH_TEXT, true)], ..Default::default() };
    h.state_mut().apply_edit(pdfcraft_engine::Edit::SetFieldProps { name: "city".into(), props: Box::new(props) });
    h.run_steps(3);
    h
}

fn field(h: &Harness<'static, PdfCraftApp>, name: &str) -> pdfcraft_engine::FormField {
    let s = h.state();
    s.session.get(s.views[0].id).unwrap().form.iter().find(|f| f.name == name).unwrap().clone()
}

fn click_field(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    let p = {
        let s = h.state();
        let doc = s.session.get(s.views[0].id).unwrap();
        let f = doc.form.iter().find(|f| f.name == name).unwrap();
        pdfcraft_ui_egui::forms_ui::field_screen_rect(&s.views[0], &doc.info, f, 0).expect("on screen").center()
    };
    h.hover_at(p);
    h.run_steps(1);
    h.drag_at(p);
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(3);
}

fn type_text(h: &mut Harness<'static, PdfCraftApp>, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.run_steps(2);
}

fn chord(h: &mut Harness<'static, PdfCraftApp>, key: Key) {
    h.key_press_modifiers(Modifiers::COMMAND, key);
    h.run_steps(2);
}

/// The committed rich value's runs as (text, bold, italic, underline, strike).
fn runs(h: &Harness<'static, PdfCraftApp>, name: &str) -> Vec<(String, bool, bool, bool, bool)> {
    let f = field(h, name);
    let Some(rv) = f.rich_value() else { return Vec::new() };
    rv.paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .map(|r| (r.text.clone(), r.style.bold == Some(true), r.style.italic == Some(true), r.style.underline, r.style.strike))
        .collect()
}

#[test]
fn command_b_i_u_format_the_selection_of_a_rich_text_field() {
    let mut h = harness();
    click_field(&mut h, "city");
    type_text(&mut h, "Paris");
    chord(&mut h, Key::A);
    chord(&mut h, Key::B);
    chord(&mut h, Key::I);
    chord(&mut h, Key::U);
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(field(&h, "city").value, ["Paris"]);
    assert_eq!(runs(&h, "city"), [("Paris".to_string(), true, true, true, false)]);
    // One undo step takes it back.
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(3);
    assert!(field(&h, "city").rich_value().is_none());
}

#[test]
fn a_style_chosen_at_the_caret_formats_what_is_typed_next() {
    let mut h = harness();
    click_field(&mut h, "city");
    type_text(&mut h, "a");
    chord(&mut h, Key::B);
    type_text(&mut h, "b");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(runs(&h, "city"), [("a".to_string(), false, false, false, false), ("b".to_string(), true, false, false, false)]);
}

#[test]
fn the_format_panel_formats_the_selection_and_keeps_the_field_open() {
    let mut h = harness();
    click_field(&mut h, "city");
    type_text(&mut h, "Lyon");
    chord(&mut h, Key::A);
    assert!(h.state().views[0].forms.focus.as_ref().is_some_and(|f| f.panel_rect.is_some()), "the panel shows");
    // A pointer click on the panel's button, as a person makes it.
    let at = h.get_by_label("Strikethrough").rect().center();
    let at = egui::pos2(at.x, at.y);
    h.hover_at(at);
    h.run_steps(1);
    h.drag_at(at);
    h.run_steps(1);
    h.drop_at(at);
    h.run_steps(4);
    assert!(h.state().views[0].forms.focus.is_some(), "the field is still being filled");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(runs(&h, "city"), [("Lyon".to_string(), false, false, false, true)]);
}

#[test]
fn an_ordinary_field_takes_no_formatting() {
    let mut h = harness();
    click_field(&mut h, "name");
    type_text(&mut h, "Ada");
    chord(&mut h, Key::A);
    assert!(h.state().views[0].forms.focus.as_ref().is_some_and(|f| f.panel_rect.is_none() && f.rich.is_none()), "no panel, no styles");
    // ⌘B does what it does elsewhere (New bookmark), which commits the field as typed.
    chord(&mut h, Key::B);
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(field(&h, "name").value, ["Ada"]);
    assert!(field(&h, "name").rich.is_none());
}

/// A one-page PDF with a multiline rich text field `desc` whose `/V` separates paragraphs with
/// carriage returns, as Acrobat writes them, and whose `/RV` makes only the first paragraph bold.
fn acrobat_style_form() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>".into(),
        "<< /Fields [5 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 6 0 R >> >> >>".into(),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (desc) /Ff {} /Rect [40 200 260 320] /P 3 0 R /DA (/Helv 10 Tf 0 g) /V (Head\\rBody one\\rBody two) /RV (<body><p><b>Head</b></p><p>Body one</p><p>Body two</p></body>) >>",
            field_flags::RICH_TEXT | field_flags::MULTILINE
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// Clicking into a field another app filled with formatted paragraphs keeps them: each on its own
/// line, only the first bold; leaving it unchanged changes nothing.
#[test]
fn opening_a_field_with_carriage_return_paragraphs_keeps_its_lines_and_styles() {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("acrobat.pdf", None, acrobat_style_form()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "150").unwrap();
        app
    });
    crate::support::settle(&mut h);
    assert!(field(&h, "desc").rich_value().is_some(), "the rich value reads");
    click_field(&mut h, "desc");
    let fx = h.state().views[0].forms.focus.clone().expect("the editor opened");
    assert_eq!(fx.text, "Head\nBody one\nBody two", "one paragraph per line");
    let draft = fx.rich.expect("a rich draft");
    let bold: Vec<bool> = draft.styles.iter().map(|s| s.bold == Some(true)).collect();
    assert_eq!(bold.iter().filter(|b| **b).count(), 4, "only Head is bold: {bold:?}");
    // Clicking away from it unchanged commits nothing: no edit, the rich value as it was.
    let away = {
        let s = h.state();
        let doc = s.session.get(s.views[0].id).unwrap();
        let f = doc.form.iter().find(|f| f.name == "desc").unwrap();
        pdfcraft_ui_egui::forms_ui::field_screen_rect(&s.views[0], &doc.info, f, 0).unwrap().center_top() - egui::vec2(0.0, 60.0)
    };
    h.hover_at(away);
    h.run_steps(1);
    h.drag_at(away);
    h.run_steps(1);
    h.drop_at(away);
    h.run_steps(4);
    assert!(h.state().views[0].forms.focus.is_none(), "the editor closed");
    let s = h.state();
    assert_eq!(s.session.get(s.views[0].id).unwrap().can_undo(), None, "nothing was changed");
    let f = field(&h, "desc");
    assert_eq!(f.rich_value().map(|rv| rv.paragraphs.len()), Some(3));
}
