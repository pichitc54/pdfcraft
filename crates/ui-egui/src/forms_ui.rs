//! Filling in forms on the page (execution plan M6; Acrobat fills fields in every viewing mode).
//!
//! Text fields open an in-place editor over the widget (Enter or clicking away commits, Escape
//! cancels, Tab moves to the next field). Check boxes and radio buttons toggle on click, or on
//! Space once Tab or a click has focused them. List boxes are directly selectable in-place on the
//! page (clicking picks an option; Ctrl-click toggles on multi-select lists). Combo boxes open a
//! list of their options; an editable one (Edit flag) also takes typed text in a box over the
//! field. Every change is one undoable engine edit.

use std::collections::HashSet;

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, vec2};
use pdfcraft_engine::{Edit, FieldValue, FormField, FormFieldKind, FormWidget, field_flags, form_appearance};
use pdfcraft_render::DocInfo;

use crate::canvas::{DocView, PageXform};
use crate::page_pointer::PagePointer;
use crate::theme::Tokens;

const FOCUS_BLUE: Color32 = Color32::from_rgb(0x14, 0x73, 0xE6);
/// Background of the in-place text field editor, the same in every theme.
const FIELD_FILL: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xF4);

/// The field being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub name: String,
    /// Which of the field's widgets (a field can appear on several pages).
    pub widget: usize,
    /// Text being typed (text fields).
    pub text: String,
    /// Choices picked so far (list boxes with multiple selection).
    pub picked: Vec<String>,
    pub request_focus: bool,
    /// Select the whole value when the editor opens (entered with Tab), so typing replaces it.
    pub select_all: bool,
    /// Where the press that opened the editor landed: the editor puts its caret there and takes
    /// over the drag, so press-and-drag selects text in a field that was not focused (#2213).
    pub press: Option<egui::Pos2>,
    /// Date fields: the month the calendar shows (year, month 1–12), and where it was drawn.
    pub calendar: Option<(i32, u32)>,
    pub calendar_rect: Option<egui::Rect>,
    /// The text the calendar last saw, so it moves to a typed date once, when the typing becomes
    /// a whole date, and otherwise keeps the month its arrows turned to.
    pub calendar_text: Option<String>,
    /// Whether the text box had the keyboard at the end of the editor's last frame
    /// ([`crate::edit_text_ui::lost_focus_once`]).
    pub(crate) had_focus: bool,
    /// Text fields that allow rich text formatting: the style of every character typed.
    pub rich: Option<crate::rich_field_ui::RichDraft>,
    /// Where the Format text panel of a rich text field was drawn: clicks there keep it open.
    pub panel_rect: Option<egui::Rect>,
}

#[derive(Clone, Debug, Default)]
pub struct FormView {
    pub focus: Option<Focus>,
    /// The draft whose edit is queued (`DocView::pending_edit`): if the field refuses the value,
    /// the editor reopens with it rather than losing the typing.
    pub(crate) committed: Option<Focus>,
    /// A press on a text field committed the field before it (#2213). Until the button is
    /// released, that field losing focus to the same gesture must not commit it again: a
    /// refused value would be validated (and alerted) twice.
    pub(crate) pressed: bool,
    /// Fields modified by the user whose appearance stream has not been rasterized by the render worker yet.
    /// Tracked as (field_name, page_index) so committed edits stay immediately visible without flicker.
    pub unbaked: HashSet<(String, usize)>,
    /// A message for the app to show (e.g. "buttons run JavaScript").
    pub notice: Option<FormNotice>,
    /// A push button was clicked: (its field name, what it does).
    pub button: Option<(String, pdfcraft_engine::form_scripts::ButtonAction)>,
    /// The focused check box or radio button was clicked. It toggles once no other edit is
    /// queued: the text the click committed goes first, and the toggle is dropped if that text
    /// is refused and its field reopens.
    pub(crate) toggle: bool,
    /// The widget holding egui's keyboard focus for the focused check box or radio button, and
    /// whether it takes the arrows (radio buttons). The page keys still work around it.
    pub(crate) keys: Option<(egui::Id, bool)>,
}

/// A form message for the app to show, with document data kept separate so the visible
/// text follows the UI language.
#[derive(Clone, Debug)]
pub enum FormNotice {
    /// Filling is blocked by the document's security settings.
    Security,
    /// A read-only field was clicked (its name).
    ReadOnly(String),
    /// A push button with no action was clicked (its name).
    NoAction(String),
}

fn widget_rect(xf: &PageXform, info: &DocInfo, page: usize, r: [f64; 4]) -> Rect {
    xf.user_rect(info, page, [r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32])
}

fn fillable(f: &FormField) -> bool {
    !f.read_only() && !matches!(f.kind, FormFieldKind::PushButton | FormFieldKind::Signature)
}

/// Whether a combo box takes typed text as well as a choice from its list: the Edit flag
/// (ISO 32000-2 Table 233), which only means something with Combo set.
fn editable_combo(f: &FormField) -> bool {
    f.kind == FormFieldKind::Combo && f.has(field_flags::EDIT)
}

/// The text a field's editor starts with: its value, or for a combo box the shown text of the
/// option it picks (a free value typed into an editable one has no option, and shows as is).
pub(crate) fn draft_text(f: &FormField) -> String {
    let value = f.value.first().cloned().unwrap_or_default();
    if f.kind == FormFieldKind::Combo
        && let Some((_, shown)) = f.options.iter().find(|(export, _)| *export == value)
    {
        return shown.clone();
    }
    value
}

fn open_focus(f: &FormField, widget: usize) -> Focus {
    Focus {
        name: f.name.clone(),
        widget,
        text: draft_text(f),
        picked: f.value.clone(),
        request_focus: true,
        select_all: false,
        press: None,
        calendar: None,
        calendar_rect: None,
        calendar_text: None,
        had_focus: false,
        rich: f.allows_rich().then(|| crate::rich_field_ui::RichDraft::of(f)),
        panel_rect: None,
    }
}

/// Select a radio button, if it isn't selected already.
fn select_edit(f: &FormField, w: &FormWidget) -> Option<Edit> {
    (w.on_state.as_ref() != f.value.first()).then(|| Edit::SetFieldValue { name: f.name.clone(), value: FieldValue::Radio(w.on_state.clone()) })
}

/// What clicking a check box or radio button does.
fn toggle_edit(f: &FormField, w: &FormWidget) -> Option<Edit> {
    let value = match f.kind {
        FormFieldKind::CheckBox => FieldValue::Check(f.value.is_empty()),
        FormFieldKind::Radio => {
            let on = w.on_state.clone();
            // Clicking the selected button again turns it off unless the group forbids it.
            let value = if f.value.first() == on.as_ref() && !f.has(field_flags::NO_TOGGLE_TO_OFF) { None } else { on };
            if value.as_ref() == f.value.first() {
                return None;
            }
            FieldValue::Radio(value)
        }
        _ => return None,
    };
    Some(Edit::SetFieldValue { name: f.name.clone(), value })
}

/// Where the pointer is relative to one page's form widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldPointer {
    /// Not over a field.
    Away,
    /// Over a field, which has set the cursor. Nothing is pressed, so the layers below still see
    /// the hover, but they must not replace the cursor: a check box drawn with a glyph, or set
    /// on printed text, is a button, not text to select.
    Over,
    /// The field owns the press or click.
    Owned,
}

/// Clicks on form widgets of one page.
#[allow(clippy::too_many_arguments)]
pub(crate) fn page_input(
    ui: &egui::Ui,
    resp: &egui::Response,
    xf: &PageXform,
    ptr: &PagePointer,
    page: usize,
    info: &DocInfo,
    form: &[FormField],
    allowed: bool,
    view: &mut DocView,
) -> FieldPointer {
    let Some(p) = ptr.within(xf.rect) else { return FieldPointer::Away };
    let hit = form.iter().find_map(|f| {
        f.widgets
            .iter()
            .enumerate()
            .find(|(_, w)| w.page == Some(page) && !w.hidden && widget_rect(xf, info, page, w.rect).contains(p))
            .map(|(i, w)| (f, i, w))
    });
    let Some((f, wi, w)) = hit else { return FieldPointer::Away };
    // The field's tooltip (`/TU`), as Acrobat shows it on hover (#3854). ISO 32000-2 Table 226
    // makes it the name the user interface identifies the field by.
    if let Some(tip) = f.tooltip.as_deref().filter(|t| !t.trim().is_empty()) {
        resp.clone().on_hover_text_at_pointer(tip);
    }
    // Empty signature fields are signed by clicking them (Use a certificate); push buttons run
    // their action.
    let pressable = f.kind == FormFieldKind::PushButton && !f.read_only() && f.button.is_some();
    // A radio button of a group can be read-only on its own.
    let usable = allowed && !w.read_only && (fillable(f) || pressable || f.kind == FormFieldKind::Signature);
    // Only where the page shows: the mode bar and side panels over a field scrolled under them
    // keep their own cursor (#2755).
    if resp.contains_pointer() {
        ui.ctx().set_cursor_icon(match (usable, f.kind) {
            (false, _) => egui::CursorIcon::NotAllowed,
            (true, FormFieldKind::Text) => egui::CursorIcon::Text,
            _ => egui::CursorIcon::PointingHand,
        });
    }
    // A text field opens on the press rather than the click, so a drag that starts on it
    // selects its text (#2213).
    let pressed = f.kind == FormFieldKind::Text && usable && resp.is_pointer_button_down_on() && ptr.pressed().is_some();
    if pressed && view.forms.focus.as_ref().is_none_or(|x| x.name != f.name || x.widget != wi) {
        commit(view, form);
        view.forms.focus = Some(Focus { press: Some(p), ..open_focus(f, wi) });
        view.forms.pressed = true;
    }
    if !resp.clicked() {
        return if resp.is_pointer_button_down_on() { FieldPointer::Owned } else { FieldPointer::Over };
    }
    if !allowed {
        view.forms.notice = Some(FormNotice::Security);
        return FieldPointer::Owned;
    }
    match f.kind {
        _ if f.read_only() || w.read_only => view.forms.notice = Some(FormNotice::ReadOnly(f.name.clone())),
        FormFieldKind::PushButton => match &f.button {
            Some(a) => view.forms.button = Some((f.name.clone(), a.clone())),
            None => view.forms.notice = Some(FormNotice::NoAction(f.name.clone())),
        },
        FormFieldKind::Signature => view.sign.field = Some(f.name.clone()),
        FormFieldKind::CheckBox | FormFieldKind::Radio => {
            // The box takes the focus, as in Acrobat, so Tab goes on from it (#2223). The overlay
            // toggles it, after the edit for any text this commits.
            if view.forms.focus.as_ref().is_none_or(|x| x.name != f.name || x.widget != wi) {
                commit(view, form);
                view.forms.focus = Some(open_focus(f, wi));
            }
            view.forms.toggle = true;
        }
        // The press already committed the previous field and opened this one (above), unless
        // press and release came in one frame. Committing again would re-run a validation that
        // refused the value on the press.
        FormFieldKind::Text if ptr.pressed().is_none() => {
            // An editor opened by the press cannot be hit until a frame has laid it out: the
            // release lands on the page and would take its focus away.
            if let Some(x) = view.forms.focus.as_mut().filter(|x| x.name == f.name && x.widget == wi) {
                x.request_focus = true;
            }
        }
        FormFieldKind::Text | FormFieldKind::Combo => {
            if view.forms.focus.as_ref().is_none_or(|x| x.name != f.name || x.widget != wi) {
                commit(view, form);
                view.forms.focus = Some(open_focus(f, wi));
            }
        }
        FormFieldKind::List => {
            commit(view, form);
            view.forms.focus = Some(open_focus(f, wi));
            let r = widget_rect(xf, info, page, w.rect);
            let height_pt = if matches!(w.rotation, 90 | 270) { (w.rect[2] - w.rect[0]).max(1.0) } else { (w.rect[3] - w.rect[1]).max(1.0) };
            let zoom = (r.height() as f64) / height_pt;
            let y_pt = (p.y - r.min.y) as f64 / zoom;
            let rows = w.list.unwrap_or(form_appearance::ListRows { top: 0, inset: 2.0, row: 12.0 * 1.15, size: 12.0 });
            let picked_indices: Vec<usize> = f.value.iter().filter_map(|v| f.options.iter().position(|(e, _)| e == v)).collect();
            let top = form_appearance::ListRows::top_showing(rows.top, &picked_indices, rows.visible(height_pt), f.options.len());
            if let Some(opt_idx) = rows.option_at(top, y_pt).filter(|&i| i < f.options.len()) {
                let export = &f.options[opt_idx].0;
                let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                let shift = ui.input(|i| i.modifiers.shift);
                let multi = f.has(field_flags::MULTI_SELECT);
                let new_value = if multi && ctrl {
                    let mut picked = f.value.clone();
                    if let Some(pos) = picked.iter().position(|x| x == export) {
                        picked.remove(pos);
                    } else {
                        picked.push(export.clone());
                    }
                    FieldValue::Choice(picked)
                } else if multi && shift && !f.value.is_empty() {
                    let anchor = f.value.last().and_then(|last| f.options.iter().position(|(e, _)| e == last)).unwrap_or(opt_idx);
                    let (min, max) = (anchor.min(opt_idx), anchor.max(opt_idx));
                    let picked = f.options[min..=max].iter().map(|(e, _)| e.clone()).collect();
                    FieldValue::Choice(picked)
                } else {
                    FieldValue::Choice(vec![export.clone()])
                };
                if new_value != FieldValue::Choice(f.value.clone()) {
                    view.pending_edit = Some(Edit::SetFieldValue { name: f.name.clone(), value: new_value });
                }
            }
        }
    }
    FieldPointer::Owned
}

/// The edit the focused field's draft makes, if it differs from the field's value.
pub(crate) fn draft_edit(focus: &Focus, form: &[FormField]) -> Option<Edit> {
    let f = form.iter().find(|f| f.name == focus.name)?;
    let value = match f.kind {
        // A rich text field: its formatting too, as a rich value (plain when nothing is formatted).
        FormFieldKind::Text if focus.rich.is_some() => {
            let mut draft = focus.rich.clone()?;
            draft.sync(&focus.text);
            let rv = draft.value();
            let text_changed = f.value.first().map(String::as_str).unwrap_or("") != focus.text;
            let had = f.rich_value();
            if draft.is_plain() && had.is_none() {
                if !text_changed {
                    return None;
                }
                FieldValue::Text(focus.text.clone())
            } else if text_changed || had != Some(&rv) {
                if draft.is_plain() { FieldValue::Text(focus.text.clone()) } else { FieldValue::Rich(rv) }
            } else {
                return None;
            }
        }
        FormFieldKind::Text if f.value.first().map(String::as_str).unwrap_or("") != focus.text => FieldValue::Text(focus.text.clone()),
        // Typed into an editable combo box: a shown text picks its option, anything else is the
        // value itself (the engine maps it, and refuses free text when Edit is clear).
        FormFieldKind::Combo if editable_combo(f) && focus.text != draft_text(f) && f.value.first() != Some(&focus.text) => {
            FieldValue::Choice(if focus.text.is_empty() { Vec::new() } else { vec![focus.text.clone()] })
        }
        _ => return None,
    };
    Some(Edit::SetFieldValue { name: f.name.clone(), value })
}

/// Close the focused field's editor, queueing the edit for its draft if it differs from the
/// field's value.
pub(crate) fn commit(view: &mut DocView, form: &[FormField]) {
    let Some(focus) = view.forms.focus.take() else { return };
    if let Some(edit) = draft_edit(&focus, form) {
        view.pending_edit = Some(edit);
        view.forms.committed = Some(focus);
    }
}

/// The text box of an editable combo box, over the field. What is typed becomes the value when
/// Enter, Tab or a click away commits it; a pick from the list still sets an option.
#[allow(clippy::too_many_arguments)]
fn combo_editor(ctx: &egui::Context, view: &mut DocView, form: &[FormField], f: &FormField, w: &FormWidget, rect: Rect, zoom: f32, widget: usize) {
    let mut enter = false;
    egui::Area::new(egui::Id::new(("form-editor", view.id.0)))
        .fade_in(false)
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .constrain_to(view.viewport_rect())
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            let Some(fx) = view.forms.focus.as_mut() else { return };
            let font = (field_text_size(ui.painter(), f, w, &fx.text) * zoom).clamp(3.0, 64.0);
            // The box is pale yellow in every theme, as a text field's is (#655).
            ui.visuals_mut().text_cursor.stroke.color = crate::edit_text_ui::caret_color_on(FIELD_FILL);
            let te = egui::TextEdit::singleline(&mut fx.text)
                .desired_width((rect.width() - 6.0 * zoom + 6.0).max(1.0))
                .font(egui::FontId::proportional(font))
                .background_color(FIELD_FILL)
                .text_color(Color32::BLACK)
                .margin(egui::Margin::symmetric(3, 1))
                .id(egui::Id::new(("form-field", view.id.0, &f.name, widget)));
            let r = ui.add_sized(rect.size(), te);
            if std::mem::take(&mut fx.request_focus) {
                r.request_focus();
                // Entered with Tab: typing replaces the value.
                if std::mem::take(&mut fx.select_all)
                    && let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), r.id)
                {
                    let n = fx.text.chars().count();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                    st.store(ui.ctx(), r.id);
                }
            }
            enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
    if enter {
        commit(view, form);
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// The date picker under a focused date field: a month of days, and a Today button under them.
/// Returns the picked date, formatted with the field's own format.
fn calendar(ctx: &egui::Context, view: &mut DocView, field: egui::Rect, fmt: &str, today: (i64, u32, u32)) -> Option<String> {
    use pdfcraft_engine::form_scripts::{DateTime, format_date, parse_date};
    let fx = view.forms.focus.as_mut()?;
    let current = parse_date(&fx.text, fmt);
    // A whole date typed since the last frame moves the calendar to it; half a date, or text
    // that hasn't changed, leaves it on the month it shows (the one the arrows turned to).
    let changed = fx.calendar_text.as_deref() != Some(fx.text.as_str());
    let (mut y, mut m) = match (fx.calendar, current) {
        (Some(_), Some(d)) if changed => (d.y, d.m),
        (Some(shown), _) => shown,
        (None, Some(d)) => (d.y, d.m),
        (None, None) => (today.0 as i32, today.1),
    };
    if changed {
        fx.calendar_text = Some(fx.text.clone());
    }
    let mut picked = None;
    let area = egui::Area::new(egui::Id::new(("date-picker", view.id.0)))
        .order(egui::Order::Foreground)
        .fixed_pos(field.left_bottom() + vec2(0.0, 2.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.small_button("‹").on_hover_text(tl!("Previous month")).clicked() {
                        (y, m) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
                    }
                    ui.label(
                        egui::RichText::new(crate::i18n::fmt(
                            tl!("{month} {y}"),
                            &[("month", tl!(MONTHS[(m.clamp(1, 12) - 1) as usize])), ("y", &y.to_string())],
                        ))
                        .strong(),
                    );
                    if ui.small_button("›").on_hover_text(tl!("Next month")).clicked() {
                        // `y + 1` would overflow at `i32::MAX` (a date parsed or persisted with
                        // that year); stay put on December of the last year instead of panicking.
                        (y, m) = if m == 12 { y.checked_add(1).map_or((y, m), |next| (next, 1)) } else { (y, m + 1) };
                    }
                });
                // Weekday of the 1st (0 = Sunday), Zeller-style via days since 1970-01-01 (a Thursday).
                use chrono::Datelike;
                // The month the calendar shows: the weekday of its 1st (0 = Sunday) and its
                // length, both from chrono. A year chrono can't represent falls back to a plain
                // 31-day month starting on Sunday, which only the arrows can reach.
                let first_of = |y: i32, m: u32| chrono::NaiveDate::from_ymd_opt(y, m, 1);
                let first = first_of(y, m).map_or(0, |d| d.weekday().num_days_from_sunday() as usize);
                // `y + 1` for December would overflow at `i32::MAX`; a year chrono can't represent
                // (or that overflows here) falls back to the plain 31-day month.
                let next_y = if m == 12 { y.checked_add(1) } else { Some(y) };
                let len = match (first_of(y, m), next_y.and_then(|next| first_of(next, if m == 12 { 1 } else { m + 1 }))) {
                    (Some(a), Some(b)) => (b - a).num_days() as usize,
                    _ => 31,
                };
                let grid = egui::Grid::new(("date-grid", view.id.0)).spacing([4.0, 2.0]).show(ui, |ui| {
                    for d in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] {
                        ui.label(egui::RichText::new(tl_ctx!("calendar weekday", d)).small());
                    }
                    ui.end_row();
                    for cell in 0..(first + len).div_ceil(7) * 7 {
                        if cell >= first && cell < first + len {
                            let day = (cell - first + 1) as u32;
                            let selected = current.is_some_and(|c| (c.y, c.m, c.d) == (y, m, day));
                            let is_today = (today.0 as i32, today.1, today.2) == (y, m, day);
                            let mut text = egui::RichText::new(day.to_string());
                            if is_today {
                                text = text.strong();
                            }
                            if ui
                                .selectable_label(selected, text)
                                .on_hover_text(crate::i18n::fmt(
                                    tl!("{month} {day}, {y}"),
                                    &[
                                        ("month", tl_ctx!("calendar date", MONTHS[(m - 1) as usize])),
                                        ("day", &day.to_string()),
                                        ("y", &y.to_string()),
                                    ],
                                ))
                                .clicked()
                            {
                                picked = Some(format_date(DateTime { y, m, d: day, hh: 0, mm: 0, ss: 0 }, fmt));
                            }
                        } else {
                            ui.label("");
                        }
                        if cell % 7 == 6 {
                            ui.end_row();
                        }
                    }
                });
                // Today, whichever month the calendar shows: picked as a click on its day would
                // pick it. A year the field's date can't hold offers no button.
                if let Ok(ty) = i32::try_from(today.0) {
                    ui.add_space(4.0);
                    // As wide as the grid, so the button sits centred under it without widening the popup.
                    ui.allocate_ui_with_layout(vec2(grid.response.rect.width(), 0.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                        let mut button = ui.small_button(tl!("Today"));
                        if let Some(month) = today.1.checked_sub(1).and_then(|i| MONTHS.get(i as usize)) {
                            button = button.on_hover_text(crate::i18n::fmt(
                                tl!("{month} {day}, {y}"),
                                &[("month", tl_ctx!("calendar date", month)), ("day", &today.2.to_string()), ("y", &ty.to_string())],
                            ));
                        }
                        if button.clicked() {
                            picked = Some(format_date(DateTime { y: ty, m: today.1, d: today.2, hh: 0, mm: 0, ss: 0 }, fmt));
                        }
                    });
                }
            });
        });
    if let Some(fx) = view.forms.focus.as_mut() {
        fx.calendar = Some((y, m));
        fx.calendar_rect = Some(area.response.rect);
    }
    picked
}

/// Paint focus and hover frames on a page, the pages' `pointer` over it.
pub(crate) fn paint_page(
    painter: &egui::Painter,
    xf: &PageXform,
    page: usize,
    info: &DocInfo,
    form: &[FormField],
    view: &DocView,
    pointer: Option<Pos2>,
) {
    for f in form {
        for (wi, w) in f.widgets.iter().enumerate().filter(|(_, w)| w.page == Some(page) && !w.hidden) {
            let r = widget_rect(xf, info, page, w.rect);
            let focused = view.forms.focus.as_ref().is_some_and(|x| x.name == f.name && x.widget == wi);
            if focused {
                painter.rect_stroke(r.expand(1.0), CornerRadius::same(2), Stroke::new(2.0, FOCUS_BLUE), egui::StrokeKind::Outside);
            }
            if view.forms.unbaked.contains(&(f.name.clone(), page)) {
                paint_unbaked_widget(painter, xf, f, w, r);
            } else if fillable(f) && pointer.is_some_and(|p| r.contains(p)) {
                painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, FOCUS_BLUE.gamma_multiply(0.8)), egui::StrokeKind::Outside);
            }
        }
    }
}

/// The line pitch of a multiline field, in em: the page appearance's, for every font.
const LINE_PITCH: f32 = 1.15;

/// `text` laid out as a multiline field draws it: at `size`, wrapped at `wrap`, one line every
/// [`LINE_PITCH`] whatever the fonts (Thai lines would otherwise take the Thai font's taller height).
pub fn pitched_job(text: &str, size: f32, wrap: f32) -> egui::text::LayoutJob {
    let format = egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color: Color32::BLACK,
        line_height: Some(size * LINE_PITCH),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    job.wrap.max_width = wrap;
    job
}

/// The size, in points, a text field shows `text` at on screen: its `/DA` size, or for auto size
/// (`0 Tf`) the page appearance's fit, measured in the font the screen draws with so the text
/// stays inside the box (the interface font runs wider than the page's Helvetica).
pub fn field_text_size(painter: &egui::Painter, f: &FormField, w: &FormWidget, text: &str) -> f32 {
    let da = f.da.split_whitespace().collect::<Vec<_>>();
    let fixed = da
        .iter()
        .position(|x| *x == "Tf")
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| da.get(i))
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s > 0.0);
    let size = fixed.unwrap_or_else(|| {
        // The appearance's padding: 2 pt inside a 1 pt border.
        let pad = 3.0;
        let (width, height) = ((w.rect[2] - w.rect[0]).abs() - 2.0 * pad, (w.rect[3] - w.rect[1]).abs() - 2.0 * pad);
        // Comb cells hold one character each, so only the height limits them.
        let comb = f.has(field_flags::COMB) && f.max_len.is_some_and(|m| m > 0);
        let fit = if comb { "" } else { text };
        let font = |size: f64| egui::FontId::proportional(size as f32);
        let line_width = |size: f64| f64::from(painter.layout_no_wrap(fit.to_string(), font(size), Color32::BLACK).size().x);
        // Wrapped as the editor and the page preview wrap it.
        let wrap = width.max(1.0) as f32;
        let wrapped_height = |size: f64| f64::from(painter.layout_job(pitched_job(fit, size as f32, wrap)).size().y);
        pdfcraft_engine::field_auto_text_size_by(width, height, f.has(field_flags::MULTILINE), line_width, wrapped_height)
    });
    size.min(300.0) as f32
}

fn paint_unbaked_widget(painter: &egui::Painter, xf: &PageXform, f: &FormField, w: &FormWidget, r: Rect) {
    match f.kind {
        FormFieldKind::Text | FormFieldKind::Combo => {
            let text = f.value.first().map(String::as_str).unwrap_or("");
            painter.rect_filled(r, CornerRadius::same(1), Color32::from_rgb(0xFF, 0xFF, 0xF4));
            painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, Color32::from_gray(180)), egui::StrokeKind::Inside);
            if !text.is_empty() {
                let unrotated_w = if xf.rot % 180 == 90 { xf.ph } else { xf.pw };
                let zoom = xf.rect.width() / unrotated_w.max(1.0);
                let font_size = (field_text_size(painter, f, w, text) * zoom).clamp(3.0, 64.0);
                let display_text = if f.has(field_flags::PASSWORD) { "•".repeat(text.chars().count()) } else { text.to_string() };
                let p = painter.with_clip_rect(r);
                let (align, pos) = match f.quadding {
                    1 => (egui::Align2::CENTER_TOP, egui::pos2(r.center().x, r.min.y + 1.0)),
                    2 => (egui::Align2::RIGHT_TOP, egui::pos2(r.max.x - 3.0, r.min.y + 1.0)),
                    _ => (egui::Align2::LEFT_TOP, egui::pos2(r.min.x + 3.0, r.min.y + 1.0)),
                };
                // Multiline fields wrap at the box, as their page appearance does.
                // At the width the size was fitted to: the box less 3 pt each side, scaled.
                let wrap = if f.has(field_flags::MULTILINE) { (r.width() - 6.0 * zoom).max(1.0) } else { f32::INFINITY };
                let galley = p.layout(display_text, egui::FontId::proportional(font_size), Color32::BLACK, wrap);
                p.galley(align.anchor_size(pos, galley.size()).min, galley, Color32::BLACK);
            }
        }
        FormFieldKind::List => {
            painter.rect_filled(r, CornerRadius::same(1), Color32::from_rgb(0xFF, 0xFF, 0xF4));
            painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, Color32::from_gray(180)), egui::StrokeKind::Inside);
            let unrotated_w = if xf.rot % 180 == 90 { xf.ph } else { xf.pw };
            let zoom = (xf.rect.width() / unrotated_w.max(1.0)) as f64;
            let rows = w.list.unwrap_or(form_appearance::ListRows { top: 0, inset: 2.0, row: 12.0 * 1.15, size: 12.0 });
            let height_pt = if matches!(w.rotation, 90 | 270) { (w.rect[2] - w.rect[0]).max(1.0) } else { (w.rect[3] - w.rect[1]).max(1.0) };
            let picked_indices: Vec<usize> = f.value.iter().filter_map(|v| f.options.iter().position(|(e, _)| e == v)).collect();
            let top = form_appearance::ListRows::top_showing(rows.top, &picked_indices, rows.visible(height_pt), f.options.len());
            let font_size = (rows.size * zoom).clamp(6.0, 64.0) as f32;
            let row_h = (rows.row * zoom) as f32;
            let inset = (rows.inset * zoom) as f32;
            let p = painter.with_clip_rect(r);
            let mut y = r.min.y + inset;
            for (export, display) in f.options.iter().skip(top) {
                if y + row_h > r.max.y {
                    break;
                }
                let row_rect = Rect::from_min_size(egui::pos2(r.min.x + 1.0, y), egui::vec2(r.width() - 2.0, row_h));
                if f.value.contains(export) {
                    p.rect_filled(row_rect, CornerRadius::ZERO, Color32::from_rgb(0x99, 0xC0, 0xDC));
                }
                p.text(
                    egui::pos2(r.min.x + 3.0, y + font_size * 0.15),
                    egui::Align2::LEFT_TOP,
                    display,
                    egui::FontId::proportional(font_size),
                    Color32::BLACK,
                );
                y += row_h;
            }
        }
        FormFieldKind::CheckBox => {
            painter.rect_filled(r, CornerRadius::same(1), Color32::from_rgb(0xFF, 0xFF, 0xF4));
            painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, Color32::from_gray(180)), egui::StrokeKind::Inside);
            let checked = f.value.first().is_some_and(|v| v != "Off" && !v.is_empty());
            if checked {
                let sz = (r.height() * 0.75).clamp(8.0, 32.0);
                painter.text(r.center(), egui::Align2::CENTER_CENTER, "✔", egui::FontId::proportional(sz), Color32::BLACK);
            }
        }
        FormFieldKind::Radio => {
            painter.rect_filled(r, CornerRadius::same(1), Color32::from_rgb(0xFF, 0xFF, 0xF4));
            painter.rect_stroke(r, CornerRadius::same(1), Stroke::new(1.0, Color32::from_gray(180)), egui::StrokeKind::Inside);
            let selected = f.value.first().map(String::as_str) == w.on_state.as_deref();
            if selected {
                let radius = (r.height().min(r.width()) * 0.25).clamp(2.0, 16.0);
                painter.circle_filled(r.center(), radius, Color32::BLACK);
            }
        }
        _ => {}
    }
}

/// How the focused text field lays text out, when it is one that doesn't scroll (`DoNotScroll`):
/// its editor then takes no more than fits ([`overlay`]).
pub(crate) fn text_box(doc: &pdfcraft_engine::Document, view: &DocView, form: &[FormField]) -> Option<form_appearance::TextBox> {
    let fx = view.forms.focus.as_ref()?;
    let f = form.iter().find(|f| f.name == fx.name)?;
    let no_scroll = f.kind == FormFieldKind::Text && f.has(field_flags::DO_NOT_SCROLL);
    no_scroll.then(|| doc.field_text_box(&fx.name, fx.widget)).flatten()
}

/// The in-place editor or option list for the focused field. Returns an edit to apply.
/// `text_box`, for a text field that doesn't scroll, is how its widget lays text out: the editor
/// then takes no more than fits.
pub(crate) fn overlay(
    ctx: &egui::Context,
    view: &mut DocView,
    info: &DocInfo,
    form: &[FormField],
    text_box: Option<&form_appearance::TextBox>,
    today: (i64, u32, u32),
) -> Option<Edit> {
    let pressed = view.forms.pressed;
    if !ctx.input(|i| i.pointer.primary_down()) {
        view.forms.pressed = false;
    }
    let toggle = std::mem::take(&mut view.forms.toggle);
    view.forms.keys = None;
    let focus = view.forms.focus.clone()?;
    let Some(f) = form.iter().find(|f| f.name == focus.name) else {
        view.forms.focus = None;
        return None;
    };
    let w = f.widgets.get(focus.widget)?;
    let page = w.page?;
    let Some(xf) = view.page_xform(page) else {
        // A check box or radio button scrolled out of view lets go (it has no draft to keep):
        // taking the keyboard back when its page returns would turn scrolling arrows into
        // choices, or pull the keyboard from wherever the user went meanwhile.
        if matches!(f.kind, FormFieldKind::CheckBox | FormFieldKind::Radio) {
            view.forms.focus = None;
        }
        return None;
    };
    let rect = widget_rect(&xf, info, page, w.rect);
    // Whether a click this frame landed outside the field. Where it landed is `interact_pos`: a
    // tap's release comes with `PointerGone` (egui-winit sends one with every touch release, and
    // winit reports Windows pen input as touch), which clears `hover_pos`.
    let clicked_outside = || ctx.input(|i| i.pointer.any_click() && !i.pointer.interact_pos().is_some_and(|p| rect.contains(p)));
    let zoom = xf.zoom();
    let t = Tokens::get(ctx);
    let mut edit = None;
    let mut close = false;
    let mut next: Option<bool> = None; // Tab / Shift+Tab
    match f.kind {
        FormFieldKind::Text => {
            let multiline = f.has(field_flags::MULTILINE);
            // Shown at once, like the paragraph editor: a fade-in shows the field's value through it.
            // Over the page, so only where the document shows: a field scrolled under the mode bar
            // or a side panel must leave them their pointer (#2755). It stays on its field, cut off
            // as the page is, rather than pushed back into the window.
            egui::Area::new(egui::Id::new(("form-editor", view.id.0)))
                .fade_in(false)
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .constrain_to(view.viewport_rect())
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_min_size(rect.size());
                    let Some(fx) = view.forms.focus.as_mut() else { return };
                    // Sized from the text being typed, so an auto-sized field shrinks to keep it in view.
                    let font = (field_text_size(ui.painter(), f, w, &fx.text) * zoom).clamp(3.0, 64.0);
                    // The field is pale yellow in every theme, so the caret must not follow the theme (pale in dark mode) (#655).
                    ui.visuals_mut().text_cursor.stroke.color = crate::edit_text_ui::caret_color_on(FIELD_FILL);
                    // A rich text field: ⌘B, ⌘I and ⌘U toggle the selection's style (area E).
                    let field_id = egui::Id::new(("form-field", view.id.0, &f.name, focus.widget));
                    let rich_look = crate::rich_field_ui::FieldLook::of(f, f64::from(font / zoom.max(1e-3)));
                    if let Some(draft) = fx.rich.as_mut() {
                        let range = selection(ui.ctx(), field_id, &fx.text);
                        for (key, what) in [
                            (egui::Key::B, crate::rich_field_ui::Toggle::Bold),
                            (egui::Key::I, crate::rich_field_ui::Toggle::Italic),
                            (egui::Key::U, crate::rich_field_ui::Toggle::Underline),
                        ] {
                            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, key)) {
                                draft.toggle(&range, what, rich_look.bold, rich_look.italic);
                            }
                        }
                    }
                    let rich_draft = fx.rich.clone();
                    // A field that doesn't scroll takes no more than fits (ISO 32000-2 Table 228).
                    let before = text_box.map(|_| fx.text.clone());
                    let mut te = if multiline { egui::TextEdit::multiline(&mut fx.text) } else { egui::TextEdit::singleline(&mut fx.text) };
                    te = te
                        // Wraps (inside the 3 px side margins) at the width the size was fitted to:
                        // the box less 3 pt each side, scaled.
                        .desired_width((rect.width() - 6.0 * zoom + 6.0).max(1.0))
                        .font(egui::FontId::proportional(font))
                        .background_color(FIELD_FILL)
                        .text_color(Color32::BLACK)
                        .margin(egui::Margin::symmetric(3, 1))
                        .password(f.has(field_flags::PASSWORD))
                        .id(egui::Id::new(("form-field", view.id.0, &f.name, focus.widget)));
                    // Multiline text keeps the page's line pitch, so its rows stay level with the
                    // fields beside it while it is typed.
                    let mut pitched = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                        ui.fonts_mut(|f| f.layout_job(pitched_job(text.as_str(), font, wrap)))
                    };
                    // A rich value shows its runs' styles as it is typed.
                    let mut styled = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                        let mut draft = rich_draft.clone().unwrap_or_default();
                        draft.sync(text.as_str());
                        ui.fonts_mut(|f| f.layout_job(crate::rich_field_ui::job(&draft, text.as_str(), &rich_look, font, wrap)))
                    };
                    if multiline {
                        te = te.desired_rows(((rect.height() / (font * LINE_PITCH)).floor() as usize).max(1));
                    }
                    if rich_draft.is_some() {
                        te = te.layouter(&mut styled);
                    } else if multiline && !f.has(field_flags::PASSWORD) {
                        te = te.layouter(&mut pitched);
                    }
                    let mut laid_out = None;
                    let r = ui.add_sized(rect.size(), |ui: &mut egui::Ui| {
                        let out = te.show(ui);
                        laid_out = Some((out.galley, out.galley_pos));
                        out.response.response
                    });
                    if let (Some(tb), Some(before)) = (text_box, before)
                        && fx.text != before
                    {
                        let (kept, caret) = tb.fitted_edit(&before, &fx.text);
                        if kept != fx.text {
                            fx.text = kept;
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
                                st.store(ui.ctx(), r.id);
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                    // MaxLen counts grapheme clusters (pdfcraft_engine::field_len; ที่ is one), which
                    // egui's char_limit can't: the text is clipped after each edit instead.
                    if let Some(max) = f.max_len {
                        let clipped = pdfcraft_engine::clip_to_max_len(&fx.text, max).len();
                        fx.text.truncate(clipped);
                    }
                    if let Some(draft) = fx.rich.as_mut() {
                        draft.sync(&fx.text);
                    }
                    if let Some(press) = fx.press.take()
                        && let Some((galley, pos)) = laid_out
                        && let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), r.id)
                    {
                        // The press went to the page, not to this editor, which did not exist yet.
                        let at = galley.cursor_from_pos(press - pos + vec2(galley.rect.left(), 0.0));
                        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(at)));
                        st.store(ui.ctx(), r.id);
                        if ui.input(|i| i.pointer.primary_down()) {
                            ui.ctx().set_dragged_id(r.id);
                        }
                    }
                    if fx.request_focus {
                        r.request_focus();
                        fx.request_focus = false;
                        if std::mem::take(&mut fx.select_all)
                            && let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), r.id)
                        {
                            let n = fx.text.chars().count();
                            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                            st.store(ui.ctx(), r.id);
                        }
                    }
                    let lost = crate::edit_text_ui::lost_focus_once(&r, &mut fx.had_focus);
                    let (enter, esc, tab, shift) = ui.input(|i| {
                        (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Tab), i.modifiers.shift)
                    });
                    if esc {
                        close = true;
                    } else if tab {
                        next = Some(!shift);
                    } else if lost
                        && (!multiline || !enter)
                        && !fx.calendar_rect.is_some_and(|c| ui.input(|i| i.pointer.interact_pos().is_some_and(|p| c.contains(p))))
                        // A press on a rich text field's Format text panel, or in its lists, keeps it open.
                        && !(fx.rich.is_some()
                            && !enter
                            && (egui::Popup::is_any_open(ui.ctx())
                                || fx.panel_rect.is_some_and(|c| {
                                    ui.input(|i| (i.pointer.any_pressed() || i.pointer.any_down() || i.pointer.any_released()) && i.pointer.interact_pos().is_some_and(|p| c.contains(p)))
                                })))
                    {
                        if pressed {
                            // The field reopened with a refused value; keep it open and focused.
                            fx.request_focus = true;
                        } else {
                            // Enter in a single-line field, or clicking elsewhere, commits.
                            commit(view, form);
                        }
                    }
                });
            // Rich text fields: the Format text panel under the field, for its selection.
            if view.forms.focus.as_ref().is_some_and(|fx| fx.rich.is_some()) {
                rich_panel(ctx, view, f, focus.widget, rect, zoom);
            }
            // Date fields: a calendar under the field (Acrobat's date picker).
            if let pdfcraft_engine::form_scripts::Format::Date(fmt) = &f.actions.format
                && let Some(picked) = calendar(ctx, view, rect, fmt, today)
            {
                if let Some(fx) = view.forms.focus.as_mut() {
                    fx.text = picked;
                }
                commit(view, form);
                return view.pending_edit.take();
            }
        }
        FormFieldKind::Combo => {
            // An editable combo box has a text box over the field as well as its list (ISO
            // 32000-2 Table 233, Edit).
            if editable_combo(f) {
                combo_editor(ctx, view, form, f, w, rect, zoom, focus.widget);
            }
            let resp = egui::Area::new(egui::Id::new(("form-choices", view.id.0)))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.left_bottom() + vec2(0.0, 2.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(rect.width().max(160.0));
                        ui.label(egui::RichText::new(&f.name).small().color(t.text_faint));
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            for (export, display) in &f.options {
                                let on = f.value.first() == Some(export);
                                if ui.selectable_label(on, display).clicked() {
                                    edit = Some(Edit::SetFieldValue { name: f.name.clone(), value: FieldValue::Choice(vec![export.clone()]) });
                                    close = true;
                                }
                            }
                        });
                    });
                });
            let clicked_away = !resp.response.contains_pointer() && clicked_outside();
            let (tab, shift) = ctx.input(|i| (i.key_pressed(egui::Key::Tab), i.modifiers.shift));
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            } else if tab {
                next = Some(!shift);
            } else if clicked_away {
                commit(view, form);
                close = true;
            }
        }
        FormFieldKind::List => {
            let (up, down, esc, tab, shift) = ctx.input_mut(|i| {
                (
                    // Consumed, so the arrows move the selection without also scrolling the page.
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::Tab),
                    i.modifiers.shift,
                )
            });
            if esc {
                close = true;
            } else if tab {
                next = Some(!shift);
            } else if (up || down) && !f.options.is_empty() {
                let current_idx = f.value.last().and_then(|v| f.options.iter().position(|(e, _)| e == v)).unwrap_or(0);
                let next_idx = if up { current_idx.saturating_sub(1) } else { (current_idx + 1).min(f.options.len().saturating_sub(1)) };
                if next_idx != current_idx && next_idx < f.options.len() {
                    let export = f.options[next_idx].0.clone();
                    edit = Some(Edit::SetFieldValue { name: f.name.clone(), value: FieldValue::Choice(vec![export]) });
                }
            }
            if clicked_outside() {
                close = true;
            }
        }
        FormFieldKind::CheckBox | FormFieldKind::Radio => {
            // Nothing is drawn over the box, but a widget there holds egui's keyboard focus, so
            // Space, Tab and the arrows come here instead of turning or scrolling the page.
            let id = egui::Id::new(("form-field", view.id.0, &f.name, focus.widget));
            let mut has_focus = false;
            // Not interactable, so clicks still reach the page under it.
            let area = egui::Area::new(egui::Id::new(("form-toggle", view.id.0))).order(egui::Order::Foreground).interactable(false);
            area.fixed_pos(rect.min).show(ctx, |ui| {
                let r = ui.interact(rect, id, egui::Sense::focusable_noninteractive());
                if let Some(fx) = view.forms.focus.as_mut()
                    && std::mem::take(&mut fx.request_focus)
                {
                    r.request_focus();
                }
                let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false };
                ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));
                has_focus = r.has_focus();
            });
            view.forms.keys = Some((id, f.kind == FormFieldKind::Radio));
            let (esc, tab, shift) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Tab), i.modifiers.shift));
            let space = has_focus && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space));
            let arrow = has_focus && f.kind == FormFieldKind::Radio;
            let step = ctx.input(|i| {
                if !arrow {
                    None
                } else if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowDown) {
                    Some(true)
                } else if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowUp) {
                    Some(false)
                } else {
                    None
                }
            });
            if esc || clicked_outside() {
                close = true;
            } else if tab {
                next = Some(!shift);
            } else if toggle && view.pending_edit.is_some() {
                view.forms.toggle = true;
            } else if toggle {
                edit = toggle_edit(f, w);
            } else if space {
                // Space toggles a check box and selects a radio button, as in browsers: it never
                // turns the selected button off.
                edit = if f.kind == FormFieldKind::Radio { select_edit(f, w) } else { toggle_edit(f, w) };
            } else if let Some(forward) = step {
                // The arrows move to the group's next or previous button and select it, wrapping.
                let stops: Vec<usize> = f.widgets.iter().enumerate().filter(|(_, w)| w.page.is_some() && !w.hidden).map(|(i, _)| i).collect();
                if let Some(pos) = stops.iter().position(|&i| i == focus.widget) {
                    let k = if forward { (pos + 1) % stops.len() } else { (pos + stops.len() - 1) % stops.len() };
                    if let Some((wi, target)) = stops.get(k).and_then(|&wi| Some((wi, f.widgets.get(wi)?))) {
                        view.forms.focus = Some(open_focus(f, wi));
                        edit = select_edit(f, target);
                        if let Some(p) = target.page
                            && view.page_screen_rect(p).is_none()
                        {
                            view.go_to_page(p);
                        }
                    }
                }
            }
        }
        _ => close = true,
    }
    if close {
        view.forms.focus = None;
    }
    if let Some(forward) = next {
        commit(view, form);
        let pending = view.pending_edit.take();
        // The next field that can be filled in, check boxes and radio buttons included (#2223), in
        // the document's tab order (each page's /Tabs: rows, columns or structure), wrapping.
        let mut order: Vec<(usize, &FormField, usize)> = form
            .iter()
            .filter(|x| fillable(x))
            .flat_map(|x| x.widgets.iter().enumerate().filter(|(_, w)| w.page.is_some() && !w.hidden).map(move |(wi, w)| (w.tab, x, wi)))
            .collect();
        order.sort_by_key(|o| o.0);
        if let Some(pos) = order
            .iter()
            .position(|(_, x, wi)| x.name == f.name && *wi == focus.widget)
            .or_else(|| order.iter().position(|(_, x, _)| x.name == f.name))
        {
            let k = if forward { (pos + 1) % order.len() } else { (pos + order.len() - 1) % order.len() };
            let (_, target, wi) = order[k];
            view.forms.focus = Some(Focus { select_all: true, ..open_focus(target, wi) });
            if let Some(p) = target.widgets[wi].page
                && view.page_screen_rect(p).is_none()
            {
                view.go_to_page(p);
            }
        }
        edit = edit.or(pending);
    }
    edit.or_else(|| view.pending_edit.take())
}

/// Where a field's widget is on screen (tests and automation).
pub fn field_screen_rect(view: &DocView, info: &DocInfo, f: &FormField, widget: usize) -> Option<Rect> {
    let w = f.widgets.get(widget)?;
    let page = w.page?;
    let xf = view.page_xform(page)?;
    Some(widget_rect(&xf, info, page, w.rect))
}

/// The editor's selection as a character range (a caret is empty), from its stored state; the
/// end of the text when it has none yet.
fn selection(ctx: &egui::Context, id: egui::Id, text: &str) -> std::ops::Range<usize> {
    let len = text.chars().count();
    egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map_or(len..len, |r| {
        let (a, b) = (usize::from(r.primary.index).min(len), usize::from(r.secondary.index).min(len));
        a.min(b)..a.max(b)
    })
}

/// The Format text panel for a rich text field's selection, under the field (above it when the
/// window has no room below): what it changes applies to the selected characters, or to what is
/// typed next at the caret.
fn rich_panel(ctx: &egui::Context, view: &mut DocView, f: &FormField, widget: usize, rect: Rect, zoom: f32) {
    use crate::format_text_ui::{FieldFormat, FontChoice, Fonts};
    use crate::rich_field_ui::{FieldLook, PanelState};
    let field_id = egui::Id::new(("form-field", view.id.0, &f.name, widget));
    let viewport = view.viewport_rect();
    let Some(fx) = view.forms.focus.as_mut() else { return };
    let Some(draft) = fx.rich.as_mut() else { return };
    let range = selection(ctx, field_id, &fx.text);
    let look = FieldLook::of(f, f64::from(field_text_size_pt(f, &fx.text, rect, zoom)));
    let before = PanelState::of(&draft.style_of(&range), draft.align_of(&range), &look);
    let mut state = PanelState::of(&draft.style_of(&range), draft.align_of(&range), &look);
    const PANEL_W: f32 = 300.0;
    const PANEL_H: f32 = 230.0;
    let below = rect.bottom() + 6.0;
    let y = if below + PANEL_H > viewport.bottom() { (rect.top() - 6.0 - PANEL_H).max(viewport.top()) } else { below };
    let x = rect.left().min(viewport.right() - PANEL_W - 6.0).max(viewport.left());
    let t = crate::theme::Tokens::get(ctx);
    let mut font = None;
    let mut changed = false;
    let shown =
        egui::Area::new(egui::Id::new(("form-rich-panel", view.id.0))).order(egui::Order::Foreground).fixed_pos(egui::pos2(x, y)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(PANEL_W);
                let mut field = FieldFormat { strike: state.strike, vertical: state.vertical, families: crate::rich_field_ui::extra_families() };
                let out = crate::format_text_ui::panel(
                    ui,
                    &t,
                    &mut state.look,
                    Some(&mut state.extras),
                    Fonts { sources: None, shown: state.shown.clone() },
                    Some(&mut field),
                );
                state.strike = field.strike;
                state.vertical = field.vertical;
                font = out.font;
                changed = out.changed;
            });
        });
    fx.panel_rect = Some(shown.response.rect);
    let family = match font {
        Some(FontChoice::Family(family)) => Some(family.label().to_string()),
        Some(FontChoice::Named(name)) => Some(name),
        _ => None,
    };
    if changed || family.is_some() {
        state.apply(&before, family, draft, &range);
        // A button or list on the panel gives the keyboard back to the field (its selection is
        // kept); a number being typed into the panel keeps it there.
        if ctx.memory(|m| m.focused()).is_none_or(|id| id == field_id) {
            fx.request_focus = true;
        }
        ctx.request_repaint();
    }
}

/// The size, in points, the field shows its text at (its `/DA` size, or the Auto size on screen).
fn field_text_size_pt(f: &FormField, text: &str, rect: Rect, zoom: f32) -> f32 {
    let toks: Vec<&str> = f.da.split_whitespace().collect();
    let fixed = toks.iter().position(|t| *t == "Tf").and_then(|i| i.checked_sub(1)).and_then(|i| toks.get(i)).and_then(|s| s.parse::<f32>().ok());
    match fixed.filter(|s| s.is_finite() && *s > 0.0) {
        Some(s) => s,
        // Auto size: about what fits the height, as the page appearance starts from.
        None => {
            let _ = text;
            ((rect.height() / zoom.max(1e-3) - 6.0) / 1.15).clamp(4.0, 12.0)
        }
    }
}
