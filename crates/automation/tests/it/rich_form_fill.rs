//! Rich values through the form tools (docs/thai-forms-spec.md area E): form_fill takes the XHTML
//! of ISO 32000-2 §12.7.4.3 for a rich text field, form_fields returns it, undo restores.

use pdfcraft_automation::{Automation, Content};
use pdfcraft_engine::{Edit, FieldProps, NewField, Session, field_flags};
use serde_json::{Value, json};

fn tool(a: &mut Automation, name: &str, args: Value) -> Result<Value, String> {
    match a.call(name, &args).map_err(|e| e.to_string())?.remove(0) {
        Content::Json(v) => Ok(v),
        other => panic!("expected JSON: {other:?}"),
    }
}

/// A form with a rich text field `rich` and an ordinary one `plain`, opened through the tools.
fn setup() -> (Automation, u64, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("pdfcraft-rich-form-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new();
    let bytes = s.create_blank(300.0, 400.0, 1).unwrap();
    let id = s.open("form.pdf", None, bytes, None).unwrap();
    for (name, y) in [("rich", 300.0), ("plain", 250.0)] {
        s.apply(id, Edit::AddField { page: 0, rect: [30.0, y, 270.0, y + 24.0], kind: NewField::Text { multiline: false }, name: Some(name.into()) })
            .unwrap();
    }
    let props = FieldProps { flags: vec![(field_flags::RICH_TEXT, true)], ..Default::default() };
    s.apply(id, Edit::SetFieldProps { name: "rich".into(), props: Box::new(props) }).unwrap();
    std::fs::write(dir.join("form.pdf"), s.save_bytes(id).unwrap().as_slice()).unwrap();
    let mut a = Automation::new().with_root(&dir).unwrap().with_clock(|| 1_700_000_000);
    let id = tool(&mut a, "doc_open", json!({"path": "form.pdf"})).unwrap()["doc"].as_u64().unwrap();
    (a, id, dir)
}

fn field(a: &mut Automation, doc: u64, name: &str) -> Value {
    let fields = tool(a, "form_fields", json!({"doc": doc})).unwrap();
    let list = fields.get("fields").and_then(Value::as_array).cloned().or_else(|| fields.as_array().cloned()).unwrap();
    list.into_iter().find(|f| f["name"] == name).unwrap()
}

#[test]
fn form_fill_takes_a_rich_value_and_form_fields_returns_it() {
    let (mut a, doc, dir) = setup();
    let xhtml = r#"<body><p>Name: <span style="font-weight:bold">Somchai</span></p></body>"#;
    tool(&mut a, "form_fill", json!({"doc": doc, "values": {"rich": {"rich": xhtml}, "plain": {"rich": xhtml}}})).unwrap();
    let rich = field(&mut a, doc, "rich");
    assert_eq!(rich["value"], "Name: Somchai");
    assert!(rich["rich"].as_str().unwrap().contains("font-weight:bold"), "{rich}");
    // An ordinary field takes only the plain text.
    let plain = field(&mut a, doc, "plain");
    assert_eq!((plain["value"].as_str(), plain.get("rich")), (Some("Name: Somchai"), None));
    // One undo step restores both.
    tool(&mut a, "edit_undo", json!({"doc": doc})).unwrap();
    assert!(field(&mut a, doc, "rich").get("rich").is_none());
    // Unreadable or misshapen rich values are refused with a message, not a crash.
    for bad in [json!({"rich": 5}), json!({"rich": "x".repeat(2 << 20)}), json!({"rich": xhtml, "other": 1})] {
        assert!(tool(&mut a, "form_fill", json!({"doc": doc, "values": {"rich": bad}})).is_err());
    }
    let _ = std::fs::remove_dir_all(dir);
}
