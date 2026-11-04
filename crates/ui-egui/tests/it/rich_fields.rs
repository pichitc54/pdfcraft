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
