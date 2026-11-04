//! Help ▸ Check for updates in this personal Thai build (docs/thai-forms-spec.md area G): never
//! asks upstream for a release, so its builds, which have no Thai form support, are never offered.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

fn release(tag: &str) -> craft_update::Release {
    craft_update::Release {
        tag: tag.to_string(),
        version: craft_update::parse_version(tag),
        html_url: format!("https://github.com/storytold/pdfcraft/releases/tag/{tag}"),
        ..Default::default()
    }
}

/// An app whose release source counts its calls and always has a newer release, with the check
/// at start turned on as the desktop app turns it on.
fn harness(calls: Arc<AtomicUsize>) -> Harness<'static, PdfCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        let calls = calls.clone();
        app.set_update_source(Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(release("v99.0.0")))
        }));
        app.set_startup_update_check(true);
        app
    })
}

#[test]
fn no_release_is_asked_for_or_offered_at_start() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut h = harness(calls.clone());
    for _ in 0..20 {
        h.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0, "upstream is never asked");
    assert!(h.query_by_label_contains("is available").is_none());
}

#[test]
fn check_for_updates_says_this_is_a_personal_build_and_downloads_nothing() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut h = harness(calls.clone());
    h.state_mut().execute("help.check_updates");
    h.run_steps(1);
    let toast = h.state().toast.as_ref().map(|(m, _)| m.clone());
    assert!(toast.as_deref().is_some_and(|m| m.contains("personal Thai build")), "{toast:?}");
    assert_eq!(h.state().last_opened_url.as_deref(), Some(pdfcraft_ui_egui::updates::RELEASES_PAGE), "upstream's release notes open");
    for _ in 0..20 {
        h.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0, "upstream is never asked");
    assert!(h.query_by_label_contains("is available").is_none());
    assert!(h.query_by_label("DOWNLOAD").is_none());
}
