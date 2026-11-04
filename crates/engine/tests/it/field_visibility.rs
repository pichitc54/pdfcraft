//! A Clear Data button set "Visible but doesn't print" shows on screen and is left out of what is
//! printed (docs/thai-forms-spec.md area J).

use std::sync::Arc;

use pdfcraft_engine::{Edit, FieldProps, FieldVisibility, NewField, Session, print};

/// The decoded streams of a PDF, joined.
fn streams(bytes: &[u8]) -> String {
    let d = pdfcraft_cos::Document::open(Arc::new(bytes.to_vec())).unwrap();
    d.object_numbers()
        .into_iter()
        .filter_map(|n| match &*d.get(pdfcraft_cos::ObjRef::new(n, d.generation(n))) {
            pdfcraft_cos::Object::Stream(s) => s.decoded().ok().map(|b| String::from_utf8_lossy(&b).into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_button_that_does_not_print_is_left_out_of_the_printout() {
    let mut s = Session::new();
    let blank = s.create_blank(595.0, 842.0, 1).unwrap();
    let id = s.open_new("form.pdf", blank).unwrap();
    s.apply(
        id,
        Edit::AddField {
            page: 0,
            rect: [480.0, 800.0, 570.0, 830.0],
            kind: NewField::Button { caption: "Clear Data".into() },
            name: Some("clear".into()),
        },
    )
    .unwrap();
    let printed = |s: &Session| streams(&s.print_pdf(id, &print::Settings { pages: vec![0], ..Default::default() }).unwrap());
    assert!(printed(&s).contains("Clear Data"), "a new button prints");
    let props = FieldProps { visibility: Some(FieldVisibility::VisibleNoPrint), ..Default::default() };
    s.apply(id, Edit::SetFieldProps { name: "clear".into(), props: Box::new(props) }).unwrap();
    assert!(!printed(&s).contains("Clear Data"), "the button is left out");
    let doc = s.get(id).unwrap();
    let f = doc.form.iter().find(|f| f.name == "clear").unwrap();
    assert_eq!(f.visibility(), FieldVisibility::VisibleNoPrint);
    assert!(!f.widgets[0].hidden, "it still shows on screen");
}
