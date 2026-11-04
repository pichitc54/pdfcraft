//! Dialogs move when dragged by their empty background, stay inside the window, and each kind
//! reopens where it was last left for the session (docs/thai-forms-spec.md area F).

use egui::accesskit::Role;
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::{Dialog, PdfCraftApp};

const SIZE: Vec2 = vec2(1400.0, 900.0);

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(SIZE).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("form.pdf", None, include_bytes!("../data/form.pdf").to_vec()).unwrap();
        app
    });
    crate::support::settle(&mut h);
    h
}

fn open(h: &mut Harness<'static, PdfCraftApp>, dialog: &str) {
    h.state_mut().set_option("dialog", dialog).unwrap();
    h.run_steps(4);
}

fn close(h: &mut Harness<'static, PdfCraftApp>) {
    h.state_mut().dialog = None;
    h.run_steps(2);
}

fn card(h: &Harness<'static, PdfCraftApp>) -> Rect {
    h.ctx.data(|d| d.get_temp::<Rect>(egui::Id::unique("dialog").with("card"))).expect("a dialog is shown")
}

/// Press at `from`, move by `by` in a few steps, release.
fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, by: Vec2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for i in 1..=4 {
        h.hover_at(from + by * (i as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(from + by);
    h.run_steps(3);
}

fn near(a: Pos2, b: Pos2) -> bool {
    (a - b).length() < 1.5
}

#[test]
fn a_dialog_dragged_by_its_background_moves_and_reopens_there() {
    let mut h = harness();
    open(&mut h, "properties");
    let before = card(&h);
    assert!(near(before.center(), pos2(SIZE.x / 2.0, SIZE.y / 2.0)), "opens centred: {before:?}");
    drag(&mut h, before.min + vec2(8.0, 6.0), vec2(-200.0, 60.0));
    let after = card(&h);
    assert!(near(after.min, before.min + vec2(-200.0, 60.0)), "{before:?} -> {after:?}");
    assert_eq!(h.state().dialog.map(|d| matches!(d, Dialog::Properties(_))), Some(true), "still open");
    close(&mut h);
    // Another kind opens centred; Properties, even on another tab, opens where it was left.
    open(&mut h, "preferences");
    assert!(near(card(&h).center(), pos2(SIZE.x / 2.0, SIZE.y / 2.0)), "Preferences centred: {:?}", card(&h));
    close(&mut h);
    open(&mut h, "custom");
    // Anchored by its centre, as a tab with a taller page grows both ways.
    assert!(near(card(&h).center(), after.center()), "Properties reopens where it was left: {:?}", card(&h));
}

#[test]
fn dragging_in_a_text_box_selects_instead_of_moving_the_dialog() {
    let mut h = harness();
    open(&mut h, "properties");
    let before = card(&h);
    let at = h
        .get_all_by_role(Role::TextInput)
        .map(|n| n.rect())
        .map(|r| pos2(r.left() + 6.0, r.center().y))
        .find(|p| before.shrink(4.0).contains(*p))
        .expect("Properties has a text box on its card");
    drag(&mut h, at, vec2(-150.0, 40.0));
    assert!(near(card(&h).min, before.min), "{before:?} -> {:?}", card(&h));
}

#[test]
fn a_dialog_stays_inside_the_window() {
    let mut h = harness();
    open(&mut h, "properties");
    let before = card(&h);
    drag(&mut h, before.min + vec2(8.0, 6.0), vec2(3000.0, 3000.0));
    let after = card(&h);
    assert!(after.right() <= SIZE.x + 0.5 && after.bottom() <= SIZE.y + 0.5, "{after:?}");
    assert!(after.right() > SIZE.x - 2.0 && after.bottom() > SIZE.y - 2.0, "pushed to the corner: {after:?}");
    // Dragging back moves it at once: the card wasn't parked off-screen.
    drag(&mut h, after.min + vec2(8.0, 6.0), vec2(-200.0, -100.0));
    assert!(near(card(&h).min, after.min + vec2(-200.0, -100.0)), "{after:?} -> {:?}", card(&h));
    // A smaller window pulls it back in.
    h.set_size(vec2(1000.0, 700.0));
    h.run_steps(4);
    let shrunk = card(&h);
    assert!(shrunk.right() <= 1000.5 && shrunk.bottom() <= 700.5, "{shrunk:?}");
}
