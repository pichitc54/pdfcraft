//! Formatting a rich text field's value while it is filled (docs/thai-forms-spec.md area E): the
//! fill editor keeps a style for every character beside its text, shows it, and commits a rich
//! value; ⌘B, ⌘I, ⌘U and the Format text panel change the selection's style.

use std::ops::Range;

use egui::{Color32, FontFamily, FontId, Stroke, text::LayoutJob};
use pdfcraft_engine::form_rich::{Align, RichParagraph, RichValue, RunStyle, StyledRun, Vertical};
use pdfcraft_engine::{FontFamily as StdFamily, FormField, TextAlign};

/// The value being typed into a rich text field: a style per character of the editor's text and
/// each paragraph's own properties (its runs empty).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichDraft {
    /// The text these styles belong to: the editor's text as last synced.
    pub text: String,
    pub styles: Vec<RunStyle>,
    pub paras: Vec<RichParagraph>,
    /// A style chosen with nothing selected: what is typed next at `.0` (a character index) takes.
    pub typing: Option<(usize, RunStyle)>,
}

impl RichDraft {
    /// The draft for field `f`'s value: its rich value when it shows the value, else plain.
    pub fn of(f: &FormField) -> Self {
        let text = f.value.first().cloned().unwrap_or_default();
        let rv = f.rich_value().cloned().unwrap_or_else(|| RichValue::plain(&text));
        Self::from_rich(&rv)
    }

    pub fn from_rich(rv: &RichValue) -> Self {
        let mut d = RichDraft::default();
        for (i, p) in rv.paragraphs.iter().enumerate() {
            if i > 0 {
                d.text.push('\n');
                d.styles.push(p.runs.first().map(|r| r.style.clone()).unwrap_or_default());
            }
            for r in &p.runs {
                d.text.push_str(&r.text);
                d.styles.extend(r.text.chars().map(|_| r.style.clone()));
            }
            d.paras.push(RichParagraph { runs: Vec::new(), ..p.clone() });
        }
        if d.paras.is_empty() {
            d.paras.push(RichParagraph::default());
        }
        d
    }

    /// The rich value the draft makes.
    pub fn value(&self) -> RichValue {
        let mut paragraphs = Vec::new();
        let mut runs: Vec<StyledRun> = Vec::new();
        let mut para = 0;
        for (c, style) in self.text.chars().zip(self.styles.iter().chain(std::iter::repeat(&RunStyle::default()))) {
            if c == '\n' {
                paragraphs.push(RichParagraph { runs: std::mem::take(&mut runs), ..self.paras.get(para).cloned().unwrap_or_default() });
                para += 1;
                continue;
            }
            match runs.last_mut() {
                Some(r) if r.style == *style => r.text.push(c),
                _ => runs.push(StyledRun { text: c.to_string(), style: style.clone() }),
            }
        }
        paragraphs.push(RichParagraph { runs, ..self.paras.get(para).cloned().unwrap_or_default() });
        RichValue { paragraphs }.normalized()
    }

    /// Whether nothing is formatted: then the value is plain text.
    pub fn is_plain(&self) -> bool {
        self.styles.iter().all(|s| *s == RunStyle::default()) && self.paras.iter().all(|p| *p == RichParagraph::default())
    }

    /// Follow the editor's text after an edit: characters it kept keep their style; inserted ones
    /// take the typing style at the caret, else the style before them (typing continues it), else
    /// the one after. Pasted text arrives the same way: in the caret's style, unformatted.
    pub fn sync(&mut self, new_text: &str) {
        if new_text == self.text {
            return;
        }
        let (old, new): (Vec<char>, Vec<char>) = (self.text.chars().collect(), new_text.chars().collect());
        let p = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let most = old.len().min(new.len()).saturating_sub(p);
        let q = old.iter().rev().zip(new.iter().rev()).take(most).take_while(|(a, b)| a == b).count();
        let (old_end, new_end) = (old.len() - q, new.len() - q);
        let inherit = match &self.typing {
            Some((at, style)) if *at == p => style.clone(),
            _ => p.checked_sub(1).and_then(|i| self.styles.get(i)).or_else(|| self.styles.get(old_end)).cloned().unwrap_or_default(),
        };
        let removed_breaks = old.get(p..old_end).unwrap_or_default().iter().filter(|c| **c == '\n').count();
        let added_breaks = new.get(p..new_end).unwrap_or_default().iter().filter(|c| **c == '\n').count();
        let inserted = new_end - p;
        let styles_end = old_end.min(self.styles.len());
        let start = p.min(styles_end);
        self.styles.splice(start..styles_end, std::iter::repeat_n(inherit, inserted));
        self.styles.resize(new.len(), RunStyle::default());
        // Paragraphs: the one the edit started in is kept; those it joined go, new ones copy it.
        let para = old.get(..p).unwrap_or_default().iter().filter(|c| **c == '\n').count();
        for _ in 0..removed_breaks {
            if self.paras.len() > para + 1 {
                self.paras.remove(para + 1);
            }
        }
        let props = self.paras.get(para).cloned().unwrap_or_default();
        for _ in 0..added_breaks {
            let at = (para + 1).min(self.paras.len());
            self.paras.insert(at, props.clone());
        }
        let count = new.iter().filter(|c| **c == '\n').count() + 1;
        self.paras.resize(count, RichParagraph::default());
        self.text = new_text.to_owned();
        // A typing style lasts while typing goes on at its caret.
        self.typing = match self.typing.take() {
            Some((at, style)) if at == p && inserted > 0 => Some((new_end, style)),
            _ => None,
        };
    }

    /// The style the panel shows for `range` (characters): its first character's, or for a caret
    /// the typing style, else the style typing there would take.
    pub fn style_of(&self, range: &Range<usize>) -> RunStyle {
        if range.is_empty() {
            if let Some((at, style)) = &self.typing
                && *at == range.start
            {
                return style.clone();
            }
            return range.start.checked_sub(1).and_then(|i| self.styles.get(i)).or_else(|| self.styles.get(range.start)).cloned().unwrap_or_default();
        }
        self.styles.get(range.start).cloned().unwrap_or_default()
    }

    /// Change the style of every character in `range`; with nothing selected, of what is typed
    /// next at the caret.
    pub fn restyle(&mut self, range: &Range<usize>, change: impl Fn(&mut RunStyle)) {
        if range.is_empty() {
            let mut style = self.style_of(range);
            change(&mut style);
            self.typing = Some((range.start, style));
            return;
        }
        let n = self.styles.len();
        for s in self.styles.get_mut(range.start.min(n)..range.end.min(n)).into_iter().flatten() {
            change(s);
        }
    }

    /// The paragraphs `range` touches, by index.
    fn paras_in(&self, range: &Range<usize>) -> Range<usize> {
        let before = |at: usize| self.text.chars().take(at).filter(|c| *c == '\n').count();
        before(range.start)..before(range.end) + 1
    }

    pub fn align_of(&self, range: &Range<usize>) -> Option<Align> {
        self.paras.get(self.paras_in(range).start).and_then(|p| p.align)
    }

    pub fn set_align(&mut self, range: &Range<usize>, align: Align) {
        let r = self.paras_in(range);
        let n = self.paras.len();
        for p in self.paras.get_mut(r.start.min(n)..r.end.min(n)).into_iter().flatten() {
            p.align = Some(align);
        }
    }

    /// Toggle bold, italic or underline on `range`, as ⌘B, ⌘I and ⌘U do: on unless its first
    /// character already has it, then off.
    pub fn toggle(&mut self, range: &Range<usize>, what: Toggle, field_bold: bool, field_italic: bool) {
        let cur = self.style_of(range);
        match what {
            Toggle::Bold => {
                let on = !cur.bold.unwrap_or(field_bold);
                self.restyle(range, |s| s.bold = Some(on));
            }
            Toggle::Italic => {
                let on = !cur.italic.unwrap_or(field_italic);
                self.restyle(range, |s| s.italic = Some(on));
            }
            Toggle::Underline => {
                let on = !cur.underline;
                self.restyle(range, |s| s.underline = on);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Bold,
    Italic,
    Underline,
}

/// The field's own look, for styles that don't say: its `/DA` size (or the size shown), colour,
/// bold and italic.
#[derive(Clone, Copy, Debug)]
pub struct FieldLook {
    pub size: f64,
    pub color: [f64; 3],
    pub bold: bool,
    pub italic: bool,
}

impl FieldLook {
    pub fn of(f: &FormField, shown_size: f64) -> Self {
        let toks: Vec<&str> = f.da.split_whitespace().collect();
        let font = toks
            .iter()
            .position(|t| *t == "Tf")
            .and_then(|i| i.checked_sub(2))
            .and_then(|i| toks.get(i))
            .map(|s| s.trim_start_matches('/').to_ascii_lowercase());
        let font = font.unwrap_or_default();
        let nums = |end: usize, n: usize| -> Option<Vec<f64>> { toks.get(end.checked_sub(n)?..end)?.iter().map(|t| t.parse().ok()).collect() };
        let mut color = [0.0; 3];
        for (i, t) in toks.iter().enumerate() {
            match *t {
                "g" => color = nums(i, 1).map_or(color, |v| [v[0]; 3]),
                "rg" => color = nums(i, 3).map_or(color, |v| [v[0], v[1], v[2]]),
                _ => {}
            }
        }
        FieldLook {
            size: shown_size,
            color,
            bold: font.contains("bo") || font.contains("bold"),
            italic: font.ends_with("it") || font.ends_with("ob") || font.contains("italic") || font.contains("oblique") || font.ends_with("bi"),
        }
    }
}

/// The text laid out as the field draws it, for the editor: each run in its style, `font` points
/// on screen per `look.size` points of the field, one line every 1.15 times its largest size.
pub fn job(draft: &RichDraft, text: &str, look: &FieldLook, font: f32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let scale = if look.size > 0.0 { font / look.size as f32 } else { 1.0 };
    // Consecutive characters of one style, by byte range.
    let mut sections: Vec<(Range<usize>, RunStyle)> = Vec::new();
    for (i, (at, c)) in text.char_indices().enumerate() {
        let style = draft.styles.get(i).cloned().unwrap_or_default();
        match sections.last_mut() {
            Some((r, s)) if *s == style => r.end = at + c.len_utf8(),
            _ => sections.push((at..at + c.len_utf8(), style)),
        }
    }
    if sections.is_empty() {
        sections.push((0..0, RunStyle::default()));
    }
    for (r, style) in sections {
        job.append(text.get(r).unwrap_or_default(), 0.0, format(&style, look, scale));
    }
    job
}

fn format(s: &RunStyle, look: &FieldLook, scale: f32) -> egui::TextFormat {
    let full = (s.size.unwrap_or(look.size) as f32 * scale).clamp(2.0, 400.0);
    let size = if s.vertical == Vertical::Normal { full } else { full * 0.58 };
    let bold = s.bold.unwrap_or(look.bold);
    let family = match s.family.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("courier" | "courier new" | "monospace") => FontFamily::Monospace,
        _ if bold => FontFamily::Name("semibold".into()),
        _ => FontFamily::Proportional,
    };
    let [r, g, b] = s.color.unwrap_or(look.color).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let color = Color32::from_rgb(r, g, b);
    let line = Stroke::new((full * 0.06).max(1.0), color);
    egui::TextFormat {
        font_id: FontId::new(size, family),
        color,
        italics: s.italic.unwrap_or(look.italic),
        underline: if s.underline { line } else { Stroke::NONE },
        strikethrough: if s.strike { line } else { Stroke::NONE },
        valign: match s.vertical {
            Vertical::Super => egui::Align::TOP,
            Vertical::Sub => egui::Align::BOTTOM,
            Vertical::Normal => egui::Align::BOTTOM,
        },
        extra_letter_spacing: s.char_spacing.unwrap_or(0.0) as f32 * scale,
        line_height: Some(full * 1.15),
        ..Default::default()
    }
}

/// The Format text panel's view of a selection's style, and the style change it makes.
pub struct PanelState {
    pub look: pdfcraft_engine::AddedText,
    pub extras: crate::edit_text_ui::Extras,
    pub strike: bool,
    pub vertical: Vertical,
    /// The family shown: a standard family's label or a family by name.
    pub shown: String,
}

impl PanelState {
    pub fn of(style: &RunStyle, align: Option<Align>, look: &FieldLook) -> Self {
        let family = match style.family.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("times" | "times new roman" | "times-roman") => StdFamily::Times,
            Some("courier" | "courier new") => StdFamily::Courier,
            _ => StdFamily::Helvetica,
        };
        let shown = style.family.clone().unwrap_or_else(|| "Helvetica".into());
        PanelState {
            look: pdfcraft_engine::AddedText {
                family,
                bold: style.bold.unwrap_or(look.bold),
                italic: style.italic.unwrap_or(look.italic),
                size: style.size.unwrap_or(look.size),
                color: style.color.unwrap_or(look.color),
                align: match align {
                    Some(Align::Center) => TextAlign::Center,
                    Some(Align::Right) => TextAlign::Right,
                    Some(Align::Justify) => TextAlign::Justify,
                    _ => TextAlign::Left,
                },
                ..Default::default()
            },
            extras: crate::edit_text_ui::Extras {
                underline: style.underline,
                line_spacing: 0.0,
                char_spacing: style.char_spacing.unwrap_or(0.0),
                scale: style.scale.unwrap_or(100.0),
            },
            strike: style.strike,
            vertical: style.vertical,
            shown,
        }
    }

    /// Apply to `draft`'s `range` what changed from `before` to `self`, and nothing else, so a
    /// mixed selection keeps its other properties; `family` is a family picked from the list.
    pub fn apply(&self, before: &PanelState, family: Option<String>, draft: &mut RichDraft, range: &Range<usize>) {
        let (a, b) = (&before.look, &self.look);
        let (ea, eb) = (&before.extras, &self.extras);
        let family = family.or_else(|| (a.family != b.family).then(|| b.family.label().to_string()));
        draft.restyle(range, |s| {
            if let Some(f) = &family {
                s.family = Some(f.clone());
            }
            if a.bold != b.bold {
                s.bold = Some(b.bold);
            }
            if a.italic != b.italic {
                s.italic = Some(b.italic);
            }
            if a.size != b.size {
                s.size = Some(b.size);
            }
            if a.color != b.color {
                s.color = Some(b.color);
            }
            if ea.underline != eb.underline {
                s.underline = eb.underline;
            }
            if ea.char_spacing != eb.char_spacing {
                s.char_spacing = (eb.char_spacing != 0.0).then_some(eb.char_spacing);
            }
            if ea.scale != eb.scale {
                s.scale = (eb.scale != 100.0).then_some(eb.scale);
            }
            if before.strike != self.strike {
                s.strike = self.strike;
            }
            if before.vertical != self.vertical {
                s.vertical = self.vertical;
            }
        });
        if a.align != b.align {
            draft.set_align(
                range,
                match b.align {
                    TextAlign::Center => Align::Center,
                    TextAlign::Right => Align::Right,
                    TextAlign::Justify => Align::Justify,
                    TextAlign::Left => Align::Left,
                },
            );
        }
    }
}

/// The families a rich text field's font list adds to the standard three: Sarabun (when found)
/// and the installed fonts whose embedding permission allows it, read once.
pub fn extra_families() -> &'static [String] {
    static FAMILIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut out = Vec::new();
        if pdfcraft_engine::thai_document_face_found() {
            out.push("Sarabun".to_string());
        }
        for name in pdfcraft_engine::embeddable_installed_families() {
            if !out.contains(&name) && !["Helvetica", "Times", "Courier"].contains(&name.as_str()) {
                out.push(name);
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(text: &str) -> RichDraft {
        RichDraft::from_rich(&RichValue::plain(text))
    }

    #[test]
    fn typing_after_a_bold_word_continues_it_and_paste_takes_the_caret_style() {
        let mut d = draft("ab cd");
        d.toggle(&(0..2), Toggle::Bold, false, false);
        d.sync("abX cd");
        assert_eq!(d.styles[2].bold, Some(true));
        d.sync("pasted abX cd");
        // At the start there is nothing before: the first character's style follows.
        assert!(d.styles.iter().take(7).all(|s| s.bold == Some(true)));
        assert_eq!(d.value().plain_text(), "pasted abX cd");
    }

    #[test]
    fn a_style_chosen_at_the_caret_applies_to_what_is_typed_next() {
        let mut d = draft("ab");
        d.toggle(&(2..2), Toggle::Italic, false, false);
        d.sync("abc");
        d.sync("abcd");
        assert_eq!((d.styles[2].italic, d.styles[3].italic), (Some(true), Some(true)));
        assert_eq!(d.styles[1].italic, None);
    }

    #[test]
    fn line_breaks_keep_paragraph_alignment() {
        let mut d = draft("one\ntwo");
        d.set_align(&(5..5), Align::Center);
        d.sync("one\ntw\no");
        assert_eq!(d.paras.iter().map(|p| p.align).collect::<Vec<_>>(), [None, Some(Align::Center), Some(Align::Center)]);
        d.sync("one two");
        assert_eq!(d.paras.len(), 1);
    }

    #[test]
    fn the_panel_changes_only_what_it_changed() {
        let look = FieldLook { size: 12.0, color: [0.0; 3], bold: false, italic: false };
        let mut d = draft("abcd");
        d.restyle(&(0..2), |s| s.italic = Some(true));
        let before = PanelState::of(&d.style_of(&(0..4)), None, &look);
        let mut after = PanelState::of(&d.style_of(&(0..4)), None, &look);
        after.look.bold = true;
        after.strike = true;
        after.apply(&before, None, &mut d, &(0..4));
        // Bold and strikethrough everywhere; italic only where it was.
        assert!(d.styles.iter().all(|s| s.bold == Some(true) && s.strike));
        assert_eq!(d.styles.iter().map(|s| s.italic).collect::<Vec<_>>(), [Some(true), Some(true), None, None]);
    }

    #[test]
    fn a_draft_round_trips_its_rich_value() {
        let rv = RichValue {
            paragraphs: vec![
                RichParagraph {
                    align: Some(Align::Right),
                    runs: vec![
                        StyledRun { text: "ชื่อ ".into(), style: RunStyle::default() },
                        StyledRun { text: "สมชาย".into(), style: RunStyle { bold: Some(true), ..Default::default() } },
                    ],
                    ..Default::default()
                },
                RichParagraph {
                    runs: vec![StyledRun { text: "x".into(), style: RunStyle { vertical: Vertical::Super, ..Default::default() } }],
                    ..Default::default()
                },
            ],
        };
        assert_eq!(RichDraft::from_rich(&rv).value(), rv);
        assert!(!RichDraft::from_rich(&rv).is_plain());
        assert!(draft("plain").is_plain());
    }
}
