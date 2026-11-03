//! Appearances of rich values (docs/thai-forms-spec.md area E): each styled run drawn in its own
//! font, size, colour, decoration, spacing, scale and position, paragraphs aligned, lines wrapped
//! at line-break opportunities (Thai between words).
//!
//! A run is drawn with one of three pens: the field's own `/DA` font (as plain values are), a
//! standard-14 face (a run's Helvetica, Times or Courier family, or a bold or italic run of the
//! field font's family), or a whole embedded TrueType face, shaped (Sarabun for Thai, and fonts
//! installed on this computer whose embedding permission allows it). Characters a run's pen
//! can't draw go to Sarabun in the run's style when it covers them. [`prepare`] embeds the faces
//! a value needs in `/DR` and records their glyphs in ToUnicode; [`draw`] only reads, finding
//! those embeddings again.

use std::collections::HashMap;

use pdfcraft_fonts::thai::{self, Source, Style, ThaiFace, ThaiFaces};
use pdfcraft_fonts::{Std14, win_ansi_encodable};

use super::*;
use crate::rich::{Align, LineHeight, RichValue, Vertical};

/// The rich value `f`'s widgets show for `text`, read from the document as it is now: only for a
/// field that takes rich values, and only while it shows the same text.
pub(super) fn rich_for(doc: &Document, f: &Field, text: &str) -> Option<RichValue> {
    if !f.allows_rich() {
        return None;
    }
    let d = doc.get(f.obj).as_dict().cloned()?;
    let rv = crate::read::rich_of(doc, &d)?;
    rv.shows(text).then_some(rv)
}

/// A standard family, for a run's family name or the field font.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StdFamily {
    Helvetica,
    Times,
    Courier,
}

fn std_face(family: StdFamily, bold: bool, italic: bool) -> Std14 {
    match (family, bold, italic) {
        (StdFamily::Helvetica, false, false) => Std14::Helvetica,
        (StdFamily::Helvetica, true, false) => Std14::HelveticaBold,
        (StdFamily::Helvetica, false, true) => Std14::HelveticaOblique,
        (StdFamily::Helvetica, true, true) => Std14::HelveticaBoldOblique,
        (StdFamily::Times, false, false) => Std14::TimesRoman,
        (StdFamily::Times, true, false) => Std14::TimesBold,
        (StdFamily::Times, false, true) => Std14::TimesItalic,
        (StdFamily::Times, true, true) => Std14::TimesBoldItalic,
        (StdFamily::Courier, false, false) => Std14::Courier,
        (StdFamily::Courier, true, false) => Std14::CourierBold,
        (StdFamily::Courier, false, true) => Std14::CourierOblique,
        (StdFamily::Courier, true, true) => Std14::CourierBoldOblique,
    }
}

/// What a run's `font-family` names.
enum FamilyKind<'a> {
    Std(StdFamily),
    Sarabun,
    Other(&'a str),
}

fn family_kind(name: &str) -> FamilyKind<'_> {
    let key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    match key.as_str() {
        "helvetica" | "helv" | "arial" | "sansserif" | "sans" => FamilyKind::Std(StdFamily::Helvetica),
        "times" | "timesroman" | "timesnewroman" | "serif" => FamilyKind::Std(StdFamily::Times),
        "courier" | "cour" | "couriernew" | "monospace" => FamilyKind::Std(StdFamily::Courier),
        "sarabun" => FamilyKind::Sarabun,
        _ => FamilyKind::Other(name),
    }
}

/// A way of drawing characters.
#[derive(Clone)]
enum Pen {
    Field,
    Std(Std14),
    Face(ThaiFace),
}

impl Pen {
    fn same(&self, other: &Pen) -> bool {
        match (self, other) {
            (Pen::Field, Pen::Field) => true,
            (Pen::Std(a), Pen::Std(b)) => a == b,
            (Pen::Face(a), Pen::Face(b)) => a.family == b.family && a.style == b.style && a.bytes.len() == b.bytes.len() && a.bytes == b.bytes,
            _ => false,
        }
    }
}

/// How the field draws: its own font and style, and the faces available.
struct Ctx<'a> {
    enc: FontText,
    font_name: String,
    font_obj: Object,
    bold: bool,
    italic: bool,
    family: StdFamily,
    faces: Option<&'a ThaiFaces>,
    /// The field font can't draw the value: runs without a family go to Sarabun (area A's rule).
    whole_sarabun: bool,
}

impl<'a> Ctx<'a> {
    fn new(doc: &Document, da: &Da, rv: &RichValue, faces: Option<&'a ThaiFaces>) -> Self {
        let text = rv.plain_text();
        let lines: Vec<&str> = text.split('\n').collect();
        let (font_name, font_obj, enc) = match dr_font(doc, &da.font).filter(|(_, enc)| lines.iter().all(|l| enc.can_encode(l))) {
            Some((o, enc)) => (da.font.clone(), o, enc),
            None => ("Helv".to_string(), helvetica(), FontText::WinAnsi),
        };
        let (bold, italic) = da_style(doc, &da.font);
        let base = dr_fonts(doc)
            .and_then(|fonts| fonts.get(da.font.as_bytes()).and_then(|o| doc.dict(o)))
            .and_then(|font| font.name(b"BaseFont").map(|n| String::from_utf8_lossy(n).into_owned()));
        let lower = base.as_deref().unwrap_or(&da.font).to_ascii_lowercase();
        let family = if lower.starts_with("ti") || lower.contains("times") {
            StdFamily::Times
        } else if lower.starts_with("co") || lower.contains("courier") {
            StdFamily::Courier
        } else {
            StdFamily::Helvetica
        };
        let field_thai = matches!(enc, FontText::Thai(_));
        let draws_all = lines.iter().all(|l| enc.first_unencodable(l).is_none());
        let sarabun_covers = faces.is_some_and(|f| thai::covers(&f.regular, &text));
        Ctx { enc, font_name, font_obj, bold, italic, family, faces, whole_sarabun: field_thai || (!draws_all && sarabun_covers) }
    }

    fn sarabun(&self, bold: bool, italic: bool) -> Option<ThaiFace> {
        self.faces.map(|f| f.for_style(bold, italic).clone())
    }

    fn can_draw(&self, pen: &Pen, g: &str) -> bool {
        match pen {
            Pen::Field => self.enc.can_encode(g) && self.enc.first_unencodable(g).is_none(),
            Pen::Std(_) => g.chars().all(win_ansi_encodable),
            Pen::Face(face) => thai::covers(face, g),
        }
    }

    /// The run split into pieces by the pen that draws each grapheme cluster.
    fn segments(&self, run: &crate::rich::StyledRun) -> Vec<(Pen, String)> {
        let bold = run.style.bold.unwrap_or(self.bold);
        let italic = run.style.italic.unwrap_or(self.italic);
        let sarabun = self.sarabun(bold, italic);
        let default = || {
            if self.whole_sarabun
                && let Some(face) = sarabun.clone()
            {
                Pen::Face(face)
            } else if (bold, italic) != (self.bold, self.italic) {
                Pen::Std(std_face(self.family, bold, italic))
            } else {
                Pen::Field
            }
        };
        let primary = match run.style.family.as_deref().map(family_kind) {
            Some(FamilyKind::Std(family)) => Pen::Std(std_face(family, bold, italic)),
            Some(FamilyKind::Sarabun) => sarabun.clone().map_or_else(|| Pen::Std(std_face(StdFamily::Helvetica, bold, italic)), Pen::Face),
            Some(FamilyKind::Other(name)) => installed_face(name, bold, italic, &run.text).map_or_else(default, Pen::Face),
            None => default(),
        };
        let mut out: Vec<(Pen, String)> = Vec::new();
        for g in run.text.graphemes(true) {
            let pen = if self.can_draw(&primary, g) {
                primary.clone()
            } else if let Some(face) = sarabun.clone().filter(|f| thai::covers(f, g)) {
                Pen::Face(face)
            } else {
                primary.clone()
            };
            match out.last_mut() {
                Some((p, s)) if p.same(&pen) => s.push_str(g),
                _ => out.push((pen, g.to_owned())),
            }
        }
        out
    }
}

/// The installed font `name` in the given style, when its embedding permission allows it and it
/// covers the run (or at least its characters that aren't Thai, which then go to Sarabun).
fn installed_face(name: &str, bold: bool, italic: bool, text: &str) -> Option<ThaiFace> {
    let index = pdfcraft_fonts::installed::index()?;
    let suffix = match (bold, italic) {
        (false, false) => "",
        (true, false) => "-Bold",
        (false, true) => "-Italic",
        (true, true) => "-BoldItalic",
    };
    let pdf_name = format!("{name}{suffix}");
    let latin: String = text.chars().filter(|c| !('\u{0e00}'..='\u{0e7f}').contains(c)).collect();
    let full = index.find(&pdf_name, text).or_else(|_| index.find(&pdf_name, &latin)).ok()?;
    let style = match (bold, italic) {
        (false, false) => Style::Regular,
        (true, false) => Style::Bold,
        (false, true) => Style::Italic,
        (true, true) => Style::BoldItalic,
    };
    Some(ThaiFace { family: full.family, style, bytes: full.bytes, index: full.index, source: Source::File(full.path) })
}

/// A pen's resource in the appearance: its name, its font object, and for shaped faces the font
/// reference (for ToUnicode). `None` for a face not embedded (it is then drawn as Helvetica).
type Resource = (String, Object, Option<ObjRef>);

/// One stretch of a line drawn with one pen and one style.
struct Frag {
    pen: usize,
    text: String,
    /// The glyph size, the run's size (for the line height), and the rise.
    size: f64,
    full: f64,
    rise: f64,
    spacing: f64,
    scale: f64,
    color: Option<[f64; 3]>,
    underline: bool,
    strike: bool,
}

/// A grapheme cluster with the style it is drawn in.
#[derive(Clone)]
struct Cell {
    g: String,
    pen: usize,
    run: (usize, usize),
}

/// A laid-out line: its fragments with their widths, its width, and its paragraph.
struct Line {
    frags: Vec<(f64, Frag)>,
    width: f64,
    para: usize,
    first_in_para: bool,
    last_in_para: bool,
}

/// The pens a rich value uses and each paragraph's cells.
fn cells(ctx: &Ctx, rv: &RichValue) -> (Vec<Pen>, Vec<Vec<Cell>>) {
    let mut pens: Vec<Pen> = Vec::new();
    let mut paras = Vec::new();
    for (pi, p) in rv.paragraphs.iter().enumerate() {
        let mut cells = Vec::new();
        for (ri, run) in p.runs.iter().enumerate() {
            for (pen, text) in ctx.segments(run) {
                let index = pens.iter().position(|q| q.same(&pen)).unwrap_or_else(|| {
                    pens.push(pen);
                    pens.len() - 1
                });
                cells.extend(text.graphemes(true).map(|g| Cell { g: g.to_owned(), pen: index, run: (pi, ri) }));
            }
        }
        paras.push(cells);
    }
    (pens, paras)
}

/// Embed the faces the rich value of `f` (as the document holds it, showing `values`) needs in
/// the form's `/DR` and record the glyphs it draws in their ToUnicode. Returns whether the value
/// is drawn as a rich value.
pub(super) fn prepare(doc: &mut Document, f: &Field, w: &Widget, values: &[String], format: bool, faces: Option<&ThaiFaces>) -> bool {
    let (_, text) = shown_values(f, values, format);
    if format && f.actions.format != crate::af::Format::None {
        return false;
    }
    let Some(rv) = rich_for(doc, f, &text) else { return false };
    let wd = doc.get(w.obj).as_dict().cloned().unwrap_or_default();
    let da = widget_da(doc, f, &wd);
    let ctx = Ctx::new(doc, &da, &rv, faces);
    let (pens, paras) = cells(&ctx, &rv);
    for (i, pen) in pens.iter().enumerate() {
        let Pen::Face(face) = pen else { continue };
        let Some((_, font)) = thai_resource(doc, face) else { continue };
        for cells in &paras {
            let text: String = cells.iter().filter(|c| c.pen == i).map(|c| c.g.as_str()).collect();
            if let Ok(run) = thai::shape(face, &text) {
                let _ = thai::add_unicode(doc, font, &run, &text);
            }
        }
    }
    true
}

/// The appearance of widget `w` of `f` showing rich value `rv`, with the first character drawn as
/// `?` and whether the text is cut off.
pub(super) fn draw(doc: &Document, f: &Field, wd: &Dict, place: Placement, da: &Da, rv: &RichValue) -> (Stream, Option<char>, Option<Overflow>) {
    let faces = thai::installed_document_faces();
    let ctx = Ctx::new(doc, da, rv, faces);
    let (pens, paras) = cells(&ctx, rv);
    let dr = dr_fonts(doc).unwrap_or_default();
    let resources: Vec<Option<Resource>> = pens
        .iter()
        .map(|pen| match pen {
            Pen::Field => Some((ctx.font_name.clone(), ctx.font_obj.clone(), None)),
            Pen::Std(s) => {
                let mut font = Dict::new();
                font.set(b"Type".to_vec(), Object::name("Font"));
                font.set(b"Subtype".to_vec(), Object::name("Type1"));
                font.set(b"BaseFont".to_vec(), Object::name(s.base_font()));
                font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
                Some((format!("PC{}", s.base_font().replace('-', "")), Object::Dict(font), None))
            }
            Pen::Face(face) => {
                thai::find_embedded(doc, &dr, face).map(|(name, r)| (String::from_utf8_lossy(&name).into_owned(), Object::Ref(r), Some(r)))
            }
        })
        .collect();
    let (width, height) = (place.layout_w, place.layout_h);
    let (c, bw) = frame(doc, wd, width, height);
    let pad = 2.0 + bw;
    let inner_w = (width - 2.0 * pad).max(1.0);
    let multiline = f.has(flags::MULTILINE);
    let q = wd.get(b"Q").and_then(|o| doc.resolve(o).as_int()).unwrap_or(f.quadding);
    let field_align = match q {
        1 => Align::Center,
        2 => Align::Right,
        _ => Align::Left,
    };
    let helv = Pen::Std(std_face(StdFamily::Helvetica, false, false));
    // A pen that can't draw (a face no longer embedded) stands in as Helvetica.
    let pen_of = |i: usize| -> &Pen { if resources.get(i).is_some_and(Option::is_some) { pens.get(i).unwrap_or(&helv) } else { &helv } };
    let width_of = |pen: &Pen, text: &str, size: f64, spacing: f64, scale: f64| -> f64 {
        let clusters = text.graphemes(true).count() as f64;
        let base = match pen {
            Pen::Field => ctx.enc.width(text, size),
            Pen::Std(s) => s.text_width(text, size),
            Pen::Face(face) => thai::shape(face, text).map_or(0.0, |r| r.glyphs.iter().map(|g| g.advance).sum::<f64>() * size),
        };
        (base + spacing * clusters) * scale / 100.0
    };
    let frag_of = |cells: &[Cell], base: f64| -> Option<Frag> {
        let first = cells.first()?;
        let run = rv.paragraphs.get(first.run.0).and_then(|p| p.runs.get(first.run.1))?;
        let full = run.style.size.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(base).min(300.0);
        let (size, rise) = match run.style.vertical {
            Vertical::Normal => (full, 0.0),
            Vertical::Super => (full * 0.58, full * 0.33),
            Vertical::Sub => (full * 0.58, -full * 0.14),
        };
        Some(Frag {
            pen: first.pen,
            text: cells.iter().map(|c| c.g.as_str()).collect(),
            size,
            full,
            rise,
            spacing: run.style.char_spacing.filter(|v| v.is_finite()).unwrap_or(0.0).clamp(-100.0, 100.0),
            scale: run.style.scale.filter(|v| v.is_finite()).unwrap_or(100.0).clamp(1.0, 1000.0),
            color: run.style.color,
            underline: run.style.underline,
            strike: run.style.strike,
        })
    };
    // Fragments of a stretch of cells (consecutive cells of one pen and run), measured.
    let frags = |cells: &[Cell], base: f64| -> Vec<(f64, Frag)> {
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(first) = cells.get(at) {
            let end = at + cells.get(at..).unwrap_or_default().iter().take_while(|c| c.pen == first.pen && c.run == first.run).count();
            if let Some(fr) = frag_of(cells.get(at..end).unwrap_or_default(), base) {
                let w = width_of(pen_of(fr.pen), &fr.text, fr.size, fr.spacing, fr.scale);
                out.push((w, fr));
            }
            at = end.max(at + 1);
        }
        out
    };
    // Lay the value out at base size `base`.
    let layout = |base: f64| -> Vec<Line> {
        let mut lines = Vec::new();
        if !multiline {
            // One line: paragraphs joined by spaces, as a single-line field shows line breaks.
            let mut all: Vec<Cell> = Vec::new();
            for (i, p) in paras.iter().enumerate() {
                if i > 0
                    && let Some(last) = all.last().cloned()
                {
                    all.push(Cell { g: " ".into(), ..last });
                }
                all.extend(p.iter().cloned());
            }
            lines.push(placed_line(frags(&all, base), 0, true, true));
            return lines;
        }
        for (pi, cells) in paras.iter().enumerate() {
            let text: String = cells.iter().map(|c| c.g.as_str()).collect();
            // Break opportunities as cell indices.
            let breaks: std::collections::HashSet<usize> = {
                let chars: Vec<usize> = thai::line_breaks(&text);
                let mut at_char = 0usize;
                let mut starts = HashMap::new();
                for (i, c) in cells.iter().enumerate() {
                    starts.insert(at_char, i);
                    at_char += c.g.chars().count();
                }
                chars.into_iter().filter_map(|ch| starts.get(&ch).copied()).collect()
            };
            let mut pieces: Vec<std::ops::Range<usize>> = Vec::new();
            let mut start = 0;
            for i in 1..=cells.len() {
                if i == cells.len() || breaks.contains(&i) {
                    pieces.push(start..i);
                    start = i;
                }
            }
            let mut line: Vec<Cell> = Vec::new();
            let mut line_w = 0.0;
            let first = lines.len();
            for piece in pieces {
                let piece_cells = cells.get(piece).unwrap_or_default();
                let trimmed = piece_cells.iter().rposition(|c| !c.g.trim().is_empty()).map_or(0, |i| i + 1);
                let w_trim: f64 = frags(piece_cells.get(..trimmed).unwrap_or_default(), base).iter().map(|(w, _)| w).sum();
                if !line.is_empty() && line_w + w_trim > inner_w + 0.01 {
                    let fr = frags(&line, base);
                    lines.push(placed_line(fr, pi, false, false));
                    line.clear();
                    line_w = 0.0;
                }
                line_w += frags(piece_cells, base).iter().map(|(w, _)| w).sum::<f64>();
                line.extend(piece_cells.iter().cloned());
            }
            lines.push(placed_line(frags(&line, base), pi, false, false));
            if let Some(l) = lines.get_mut(first) {
                l.first_in_para = true;
            }
            if let Some(l) = lines.last_mut() {
                l.last_in_para = true;
            }
        }
        lines
    };
    // Each line's height and the first baseline, from its fragments.
    let max_size = |l: &Line, base: f64| l.frags.iter().map(|(_, f)| f.full).fold(0.0, f64::max).max(if l.frags.is_empty() { base } else { 0.0 });
    let has_face = |l: &Line| {
        l.frags
            .iter()
            .filter_map(|(_, f)| match pen_of(f.pen) {
                Pen::Face(face) => Some(face),
                _ => None,
            })
            .next()
            .cloned()
    };
    let pitch = |l: &Line, base: f64| -> f64 {
        let s = max_size(l, base);
        match rv.paragraphs.get(l.para).and_then(|p| p.line_height) {
            Some(LineHeight::Points(v)) => v,
            Some(LineHeight::Factor(v)) => s * v,
            None => s * 1.15,
        }
    };
    // Baselines of every line, and how many fit inside the border.
    let baselines = |lines: &[Line], base: f64| -> (Vec<f64>, usize) {
        let mut ys = Vec::new();
        let mut y = height - pad;
        for (i, l) in lines.iter().enumerate() {
            let s = max_size(l, base);
            let para = rv.paragraphs.get(l.para);
            if l.first_in_para && i > 0 {
                y -= para.and_then(|p| p.space_before).unwrap_or(0.0).clamp(0.0, 1000.0);
            }
            if i == 0 {
                let room = has_face(l).and_then(|face| thai::mark_top(&face)).map_or(0.0, |top| top * s - pad);
                y -= (s * 0.85).max(room);
            } else {
                y -= pitch(l, base);
            }
            ys.push(y);
            if l.last_in_para {
                y -= para.and_then(|p| p.space_after).unwrap_or(0.0).clamp(0.0, 1000.0);
            }
        }
        let room = lines.iter().zip(&ys).filter(|(l, y)| **y - 0.207 * max_size(l, base) >= bw - 0.01).count();
        (ys, room)
    };
    let fits = |base: f64| -> bool {
        let lines = layout(base);
        if multiline {
            let (_, room) = baselines(&lines, base);
            room == lines.len() && lines.iter().all(|l| l.width <= inner_w + 0.01 || l.frags.len() <= 1)
        } else {
            lines.iter().all(|l| l.width <= inner_w + 0.01 && max_size(l, base) * 1.15 <= height - 2.0 * pad + 0.01)
        }
    };
    let base = if da.size > 0.0 {
        da.size
    } else {
        let mut s = 12.0;
        while s > 4.0 && !fits(s) {
            s -= 0.5;
        }
        s
    };
    let lines = layout(base);
    let (ys, room) = if multiline {
        baselines(&lines, base)
    } else {
        // One line, centred vertically on its ascent and descent.
        let l = lines.first();
        let s = l.map_or(base, |l| max_size(l, base));
        let (a, d) = l.and_then(has_face).and_then(|face| thai::vertical_metrics(&face)).map_or((0.718, 0.207), |(a, d)| (a, -d));
        (vec![(height - (a + d) * s) / 2.0 + d * s], 1)
    };
    let mut body: Vec<u8> = Vec::new();
    let mut deco = String::new();
    let mut unencodable: Option<char> = None;
    let mut used: Vec<usize> = Vec::new();
    for (l, y) in lines.iter().zip(&ys) {
        if *y < -max_size(l, base) {
            break;
        }
        let para = rv.paragraphs.get(l.para);
        let align = para.and_then(|p| p.align).unwrap_or(field_align);
        // Justified: the room left goes to the gaps after spaces, except on a paragraph's last line.
        let spaces = l.frags.iter().map(|(_, f)| f.text.trim_end().matches(' ').count()).sum::<usize>()
            + l.frags.iter().filter(|(_, f)| f.text.ends_with(' ')).count();
        let trailing: f64 = l.frags.last().map_or(0.0, |(_, f)| {
            let t = f.text.trim_end();
            width_of(pen_of(f.pen), &f.text, f.size, f.spacing, f.scale) - width_of(pen_of(f.pen), t, f.size, f.spacing, f.scale)
        });
        let used_w = l.width - trailing;
        let (mut x, gap) = match align {
            Align::Center => (pad + (inner_w - used_w) / 2.0, 0.0),
            Align::Right => (width - pad - used_w, 0.0),
            Align::Justify if !l.last_in_para && spaces > 0 => (pad, ((inner_w - used_w) / spaces as f64).max(0.0)),
            _ => (pad, 0.0),
        };
        for (w, fr) in &l.frags {
            let pen = pen_of(fr.pen);
            let name = match resources.get(fr.pen).and_then(Option::as_ref) {
                Some((name, _, _)) => name.clone(),
                None => "PCHelvetica".into(),
            };
            if !used.contains(&fr.pen) {
                used.push(fr.pen);
            }
            let color = fr.color.map_or_else(|| da.color.clone(), |[r, g, b]| format!("{} {} {} rg", n(r), n(g), n(b)));
            let ty = y + fr.rise;
            body.extend(format!("/{name} {} Tf {color} {} Tz ", n(fr.size), n(fr.scale)).bytes());
            match pen {
                Pen::Face(face) => {
                    body.extend(format!("0 Tc 1 0 0 1 {} {} Tm\n", n(x), n(ty)).bytes());
                    if let Ok(mut run) = thai::shape(face, &fr.text) {
                        // Character spacing after each cluster, in em (Tc would also space the marks).
                        if fr.spacing != 0.0 && fr.size > 0.0 {
                            let em = fr.spacing / fr.size;
                            let len = run.glyphs.len();
                            for i in 0..len {
                                let last = run.glyphs.get(i + 1).is_none_or(|next| run.glyphs.get(i).is_some_and(|g| g.cluster != next.cluster));
                                if last && let Some(g) = run.glyphs.get_mut(i) {
                                    g.advance += em;
                                }
                            }
                        }
                        body.extend(thai::show(&run, &fr.text, fr.size));
                    }
                }
                Pen::Field => {
                    if unencodable.is_none() {
                        unencodable = ctx.enc.first_unencodable(&fr.text);
                    }
                    body.extend(format!("{} Tc 1 0 0 1 {} {} Tm ", n(fr.spacing), n(x), n(ty)).bytes());
                    body.extend(literal(&ctx.enc.encode(&fr.text)));
                    body.extend_from_slice(b" Tj\n");
                }
                Pen::Std(_) => {
                    if unencodable.is_none() {
                        unencodable = fr.text.chars().find(|c| !c.is_control() && !win_ansi_encodable(*c));
                    }
                    body.extend(format!("{} Tc 1 0 0 1 {} {} Tm ", n(fr.spacing), n(x), n(ty)).bytes());
                    body.extend(literal(&win_ansi(&fr.text)));
                    body.extend_from_slice(b" Tj\n");
                }
            }
            if fr.underline || fr.strike {
                let t = fr.text.trim_end();
                let dw = width_of(pen, t, fr.size, fr.spacing, fr.scale);
                let thick = (fr.size * 0.05).max(0.4);
                let fill = fr.color.map_or_else(|| da.color.clone(), |[r, g, b]| format!("{} {} {} rg", n(r), n(g), n(b)));
                if fr.underline {
                    // Below Thai marks under the letters (ุ ู) and Latin descenders.
                    let below = match pen {
                        Pen::Face(face) => thai::vertical_metrics(face).map_or(0.15, |(_, d)| (-d).max(0.15)),
                        _ => 0.15,
                    };
                    deco.push_str(&format!("{fill}\n{} {} {} {} re f\n", n(x), n(ty - below * fr.size - thick), n(dw), n(thick)));
                }
                if fr.strike {
                    deco.push_str(&format!("{fill}\n{} {} {} {} re f\n", n(x), n(ty + fr.size * 0.3), n(dw), n(thick)));
                }
            }
            x += w + gap * (fr.text.trim_end().matches(' ').count() + usize::from(fr.text.ends_with(' '))) as f64;
        }
    }
    let mut content = format!("/Tx BMC\nq\n{c}{} {} {} {} re W n\nBT\n", n(bw), n(bw), n(width - 2.0 * bw), n(height - 2.0 * bw)).into_bytes();
    content.extend(body);
    content.extend_from_slice(b"0 Tc 100 Tz\nET\n");
    content.extend(deco.bytes());
    content.extend_from_slice(b"Q\nEMC\n");
    let mut fonts = Dict::new();
    for i in used {
        match resources.get(i).cloned().flatten() {
            Some((name, obj, _)) => fonts.set(name.into_bytes(), obj),
            None => {
                if let Some((name, obj)) = std_resource(Std14::Helvetica) {
                    fonts.set(name.into_bytes(), obj);
                }
            }
        }
    }
    let mut res = Dict::new();
    res.set(b"Font".to_vec(), Object::Dict(fonts));
    let mut d = Dict::new();
    d.set(b"Type".to_vec(), Object::name("XObject"));
    d.set(b"Subtype".to_vec(), Object::name("Form"));
    set_form_box(&mut d, place);
    d.set(b"Resources".to_vec(), Object::Dict(res));
    let overflow = if room < lines.len() {
        Some(Overflow::Tall { lines: lines.len(), room, size: base })
    } else {
        lines.iter().map(|l| l.width).filter(|w| *w > inner_w + 0.01).reduce(f64::max).map(|w| Overflow::Wide { width: w, room: inner_w, size: base })
    };
    (Stream::flate(d, &content), unencodable, overflow.filter(|_| !rv.plain_text().is_empty()))
}

/// A standard-14 face's resource: its name and a WinAnsi font dictionary.
fn std_resource(s: Std14) -> Option<(String, Object)> {
    let mut font = Dict::new();
    font.set(b"Type".to_vec(), Object::name("Font"));
    font.set(b"Subtype".to_vec(), Object::name("Type1"));
    font.set(b"BaseFont".to_vec(), Object::name(s.base_font()));
    font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
    Some((format!("PC{}", s.base_font().replace('-', "")), Object::Dict(font)))
}

/// A line of measured fragments.
fn placed_line(frags: Vec<(f64, Frag)>, para: usize, first: bool, last: bool) -> Line {
    let width = frags.iter().map(|(w, _)| w).sum();
    Line { frags, width, para, first_in_para: first, last_in_para: last }
}
