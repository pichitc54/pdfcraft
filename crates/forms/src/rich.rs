//! Rich values of text fields (ISO 32000-2 §12.7.4.3): the formatted version of a field value,
//! `/RV`, kept beside the plain `/V` in fields that allow rich text formatting
//! (docs/thai-forms-spec.md area E).
//!
//! A rich value is XHTML: a `body` of `p` paragraphs holding text, `span`, `b` and `i`, styled by
//! `style` attributes with the CSS2 properties §12.7.4.3 lists. [`parse`] reads it into
//! [`RichValue`] (paragraphs of styled runs); [`RichValue::to_xhtml`] writes one back. Reading is
//! bounded and lenient: unknown elements are read for their text, unknown properties ignored, and
//! anything unreadable gives `None` so the plain value is shown instead.

/// Longest `/RV` read; past this the plain value is shown.
pub const MAX_RV_BYTES: usize = 1 << 20;
/// Deepest element nesting read.
const MAX_DEPTH: usize = 64;
/// Most styled runs read.
const MAX_RUNS: usize = 20_000;

/// The formatted version of a field value: its paragraphs in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichValue {
    pub paragraphs: Vec<RichParagraph>,
}

/// One paragraph of a rich value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichParagraph {
    /// `None`: the field's own quadding.
    pub align: Option<Align>,
    /// Spacing before and after, in points, and the line spacing, as another app wrote them.
    /// Shown, never applied by PdfCraft's editor.
    pub space_before: Option<f64>,
    pub space_after: Option<f64>,
    pub line_height: Option<LineHeight>,
    pub runs: Vec<StyledRun>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

/// A paragraph's line spacing: an absolute distance or a multiple of the font size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Points(f64),
    Factor(f64),
}

/// A stretch of text with one style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyledRun {
    pub text: String,
    pub style: RunStyle,
}

/// A styled run's style. `None` (or `false` for the decorations) inherits the field's `/DA`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunStyle {
    /// The first family `font-family` names, unquoted.
    pub family: Option<String>,
    /// Points.
    pub size: Option<f64>,
    /// `Some(false)` is an explicit normal weight, overriding a bold field font.
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: bool,
    pub strike: bool,
    /// RGB, each 0–1.
    pub color: Option<[f64; 3]>,
    pub vertical: Vertical,
    /// Extra space after each character, in points.
    pub char_spacing: Option<f64>,
    /// Horizontal scale in percent (100 is normal).
    pub scale: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vertical {
    #[default]
    Normal,
    Super,
    Sub,
}

impl RichValue {
    /// A rich value of plain text in one style: one paragraph per line.
    pub fn plain(text: &str) -> Self {
        let text = normalize_breaks(text);
        RichValue {
            paragraphs: text
                .split('\n')
                .map(|line| RichParagraph {
                    runs: if line.is_empty() { Vec::new() } else { vec![StyledRun { text: line.to_owned(), style: RunStyle::default() }] },
                    ..RichParagraph::default()
                })
                .collect(),
        }
    }

    /// The plain text: paragraphs joined by line breaks, as `/V` holds it.
    pub fn plain_text(&self) -> String {
        self.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// Whether this rich value shows `text` (line breaks compared loosely: `\r\n`, `\r`, `\n`).
    pub fn shows(&self, text: &str) -> bool {
        self.plain_text() == normalize_breaks(text)
    }

    /// Adjacent runs of one style merged and empty runs dropped, so equal values compare equal.
    pub fn normalized(mut self) -> Self {
        for p in &mut self.paragraphs {
            let mut runs: Vec<StyledRun> = Vec::with_capacity(p.runs.len());
            for r in p.runs.drain(..).filter(|r| !r.text.is_empty()) {
                match runs.last_mut() {
                    Some(last) if last.style == r.style => last.text.push_str(&r.text),
                    _ => runs.push(r),
                }
            }
            p.runs = runs;
        }
        self
    }

    /// The rich value as XHTML for `/RV` (§12.7.4.3).
    pub fn to_xhtml(&self) -> String {
        let mut out = String::from(
            "<?xml version=\"1.0\"?><body xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:xfa=\"http://www.xfa.org/schema/xfa-data/1.0/\" xfa:spec=\"2.0.2\">",
        );
        for p in &self.paragraphs {
            let mut css = Vec::new();
            if let Some(a) = p.align {
                css.push(format!(
                    "text-align:{}",
                    match a {
                        Align::Left => "left",
                        Align::Center => "center",
                        Align::Right => "right",
                        Align::Justify => "justify",
                    }
                ));
            }
            if let Some(v) = p.space_before {
                css.push(format!("margin-top:{}pt", num(v)));
            }
            if let Some(v) = p.space_after {
                css.push(format!("margin-bottom:{}pt", num(v)));
            }
            match p.line_height {
                Some(LineHeight::Points(v)) => css.push(format!("line-height:{}pt", num(v))),
                Some(LineHeight::Factor(v)) => css.push(format!("line-height:{}", num(v))),
                None => {}
            }
            out.push_str("<p");
            push_style(&mut out, &css);
            out.push('>');
            for r in &p.runs {
                let css = run_css(&r.style);
                if css.is_empty() {
                    push_text(&mut out, &r.text);
                } else {
                    out.push_str("<span");
                    push_style(&mut out, &css);
                    out.push('>');
                    push_text(&mut out, &r.text);
                    out.push_str("</span>");
                }
            }
            out.push_str("</p>");
        }
        out.push_str("</body>");
        out
    }
}

/// `\r\n` and `\r` as `\n`.
pub fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn run_css(s: &RunStyle) -> Vec<String> {
    let mut css = Vec::new();
    if let Some(f) = &s.family {
        css.push(format!("font-family:'{}'", f.replace(['\'', '"', ';', '<', '>', '&'], "")));
    }
    if let Some(v) = s.size {
        css.push(format!("font-size:{}pt", num(v)));
    }
    if let Some(b) = s.bold {
        css.push(format!("font-weight:{}", if b { "bold" } else { "normal" }));
    }
    if let Some(i) = s.italic {
        css.push(format!("font-style:{}", if i { "italic" } else { "normal" }));
    }
    match (s.underline, s.strike) {
        (true, true) => css.push("text-decoration:underline line-through".into()),
        (true, false) => css.push("text-decoration:underline".into()),
        (false, true) => css.push("text-decoration:line-through".into()),
        (false, false) => {}
    }
    if let Some([r, g, b]) = s.color {
        let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        css.push(format!("color:#{:02x}{:02x}{:02x}", c(r), c(g), c(b)));
    }
    match s.vertical {
        Vertical::Super => css.push("vertical-align:super".into()),
        Vertical::Sub => css.push("vertical-align:sub".into()),
        Vertical::Normal => {}
    }
    if let Some(v) = s.char_spacing {
        css.push(format!("letter-spacing:{}pt", num(v)));
    }
    if let Some(v) = s.scale {
        css.push(format!("font-stretch:{}%", num(v)));
    }
    css
}

fn push_style(out: &mut String, css: &[String]) {
    if !css.is_empty() {
        out.push_str(" style=\"");
        push_text(out, &css.join(";"));
        out.push('"');
    }
}

/// `text` with the XML special characters escaped.
fn push_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if c.is_control() && !matches!(c, '\t') => out.push_str(&format!("&#{};", u32::from(c))),
            c => out.push(c),
        }
    }
}

/// A number with at most three decimals, no trailing zeros.
fn num(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

// ── reading ─────────────────────────────────────────────────────────────────────────────────

/// What an open element contributes: the run style and paragraph properties in force inside it.
#[derive(Clone, Default)]
struct Scope {
    style: RunStyle,
    para: RichParagraph,
}

/// Read a rich value. `None` when `xhtml` is too large, too deep or has no `body`-level text
/// structure at all; the caller then shows the plain value.
pub fn parse(xhtml: &str) -> Option<RichValue> {
    if xhtml.len() > MAX_RV_BYTES {
        return None;
    }
    let mut out = RichValue::default();
    let mut stack: Vec<(String, Scope)> = vec![(String::new(), Scope::default())];
    // The paragraph text is going into; opened lazily by text outside any `p`.
    let mut open: Option<RichParagraph> = None;
    let mut runs = 0usize;
    let mut rest = xhtml;
    let mut seen_body = false;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            // Comments, processing instructions, doctypes and CDATA.
            if let Some(r) = after.strip_prefix("!--") {
                rest = r.find("-->").map_or("", |i| r.get(i + 3..).unwrap_or(""));
                continue;
            }
            if let Some(r) = after.strip_prefix("![CDATA[") {
                let end = r.find("]]>").unwrap_or(r.len());
                let text = r.get(..end).unwrap_or("");
                push_run(&mut out, &mut open, &stack, text, &mut runs)?;
                rest = r.get(end + 3..).unwrap_or("");
                continue;
            }
            if after.starts_with('?') || after.starts_with('!') {
                rest = after.find('>').map_or("", |i| after.get(i + 1..).unwrap_or(""));
                continue;
            }
            let end = tag_end(after)?;
            let tag = after.get(..end)?;
            rest = after.get(end + 1..).unwrap_or("");
            if let Some(name) = tag.strip_prefix('/') {
                let name = local(name.trim());
                if name == "p" {
                    close_paragraph(&mut out, &mut open);
                }
                // Close up to the matching element (lenient about missing end tags).
                if let Some(at) = stack.iter().rposition(|(n, _)| *n == name)
                    && at > 0
                {
                    stack.truncate(at);
                }
                continue;
            }
            let self_closing = tag.ends_with('/');
            let tag = tag.trim_end_matches('/');
            let (name, attrs) = split_tag(tag);
            let name = local(name);
            let mut scope = stack.last().map(|(_, s)| s.clone()).unwrap_or_default();
            match name.as_str() {
                "body" => {
                    seen_body = true;
                    scope.para = RichParagraph::default();
                }
                "p" | "div" => {
                    close_paragraph(&mut out, &mut open);
                    scope.para = RichParagraph::default();
                }
                "b" | "strong" => scope.style.bold = Some(true),
                "i" | "em" => scope.style.italic = Some(true),
                "br" => {
                    // A line break inside a paragraph starts the next one with the same properties.
                    let props = open.as_ref().map_or_else(|| scope.para.clone(), |p| RichParagraph { runs: Vec::new(), ..p.clone() });
                    out.paragraphs.push(open.take().unwrap_or_else(|| props.clone()));
                    open = Some(props);
                    continue;
                }
                _ => {}
            }
            if let Some(css) = attr(attrs, "style") {
                apply_css(&css, &mut scope);
            }
            if matches!(name.as_str(), "p" | "div") {
                open = Some(RichParagraph { runs: Vec::new(), ..scope.para.clone() });
            }
            if !self_closing {
                if stack.len() >= MAX_DEPTH {
                    return None;
                }
                stack.push((name, scope));
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let raw = rest.get(..end).unwrap_or("");
            rest = rest.get(end..).unwrap_or("");
            let text = unescape(raw);
            // Whitespace between block elements is markup layout, not text.
            if open.is_none() && text.trim().is_empty() {
                continue;
            }
            push_run(&mut out, &mut open, &stack, &text, &mut runs)?;
        }
    }
    close_paragraph(&mut out, &mut open);
    (seen_body || !out.paragraphs.is_empty()).then(|| out.normalized())
}

fn push_run(out: &mut RichValue, open: &mut Option<RichParagraph>, stack: &[(String, Scope)], text: &str, runs: &mut usize) -> Option<()> {
    if text.is_empty() {
        return Some(());
    }
    *runs += 1;
    if *runs > MAX_RUNS {
        return None;
    }
    let scope = stack.last().map(|(_, s)| s.clone()).unwrap_or_default();
    let para = open.get_or_insert_with(|| RichParagraph { runs: Vec::new(), ..scope.para.clone() });
    // A line break in the text is a paragraph break, as in `/V`.
    let text = normalize_breaks(text);
    let mut lines = text.split('\n');
    if let Some(first) = lines.next()
        && !first.is_empty()
    {
        para.runs.push(StyledRun { text: first.to_owned(), style: scope.style.clone() });
    }
    for line in lines {
        let next = RichParagraph { runs: Vec::new(), ..para.clone() };
        let done = std::mem::replace(para, next);
        out.paragraphs.push(done);
        if !line.is_empty() {
            para.runs.push(StyledRun { text: line.to_owned(), style: scope.style.clone() });
        }
    }
    Some(())
}

fn close_paragraph(out: &mut RichValue, open: &mut Option<RichParagraph>) {
    if let Some(p) = open.take() {
        out.paragraphs.push(p);
    }
}

/// Where the tag starting after `<` ends (its `>`), skipping quoted attribute values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn split_tag(tag: &str) -> (&str, &str) {
    let tag = tag.trim();
    match tag.find(char::is_whitespace) {
        Some(i) => (tag.get(..i).unwrap_or(tag), tag.get(i..).unwrap_or("")),
        None => (tag, ""),
    }
}

/// An element name without its namespace prefix, lowercased.
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_ascii_lowercase()
}

/// The value of attribute `want` (namespace prefix ignored), unescaped.
fn attr(attrs: &str, want: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let name = local(rest.get(..eq)?.trim());
        let after = rest.get(eq + 1..)?.trim_start();
        let q = after.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let body = after.get(1..)?;
        let end = body.find(q)?;
        if name == want {
            return Some(unescape(body.get(..end)?));
        }
        rest = body.get(end + 1..)?;
    }
    None
}

/// XML entities and character references resolved.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let after = rest.get(i + 1..).unwrap_or("");
        let end = after.find(';').filter(|e| *e <= 10);
        let decoded = end.and_then(|e| {
            let name = after.get(..e)?;
            match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => {
                    let code = if let Some(h) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                        u32::from_str_radix(h, 16).ok()
                    } else {
                        name.strip_prefix('#').and_then(|d| d.parse().ok())
                    };
                    code.and_then(char::from_u32)
                }
            }
        });
        match (decoded, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = after.get(e + 1..).unwrap_or("");
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Apply a `style` attribute's declarations to `scope`.
fn apply_css(css: &str, scope: &mut Scope) {
    for decl in css.split(';') {
        let Some((prop, value)) = decl.split_once(':') else { continue };
        let (prop, value) = (prop.trim().to_ascii_lowercase(), value.trim());
        let lower = value.to_ascii_lowercase();
        let s = &mut scope.style;
        match prop.as_str() {
            "font-family" => s.family = family(value),
            "font-size" => s.size = length(&lower, s.size).or(s.size),
            "font-weight" => s.bold = weight(&lower).or(s.bold),
            "font-style" => s.italic = italic(&lower).or(s.italic),
            "font" => shorthand(value, s),
            "color" => s.color = color(&lower).or(s.color),
            "text-decoration" => {
                if lower.contains("none") {
                    s.underline = false;
                    s.strike = false;
                }
                if lower.contains("underline") {
                    s.underline = true;
                }
                if lower.contains("line-through") {
                    s.strike = true;
                }
            }
            "vertical-align" => {
                s.vertical = if lower == "super" || lower.starts_with('+') || lower.parse::<f64>().is_ok_and(|v| v > 0.0) {
                    Vertical::Super
                } else if lower == "sub" || lower.starts_with('-') {
                    Vertical::Sub
                } else if lower == "baseline" {
                    Vertical::Normal
                } else {
                    // A measurement such as "3pt": positive raises.
                    length(&lower, None).map_or(s.vertical, |v| if v > 0.0 { Vertical::Super } else { Vertical::Normal })
                };
            }
            "letter-spacing" => s.char_spacing = if lower == "normal" { None } else { length(&lower, s.size).or(s.char_spacing) },
            "font-stretch" => s.scale = stretch(&lower).or(s.scale),
            "text-align" => {
                scope.para.align = match lower.as_str() {
                    "left" | "start" => Some(Align::Left),
                    "center" | "centre" => Some(Align::Center),
                    "right" | "end" => Some(Align::Right),
                    "justify" | "justify-all" => Some(Align::Justify),
                    _ => scope.para.align,
                }
            }
            "margin-top" | "space-before" => scope.para.space_before = length(&lower, s.size).or(scope.para.space_before),
            "margin-bottom" | "space-after" => scope.para.space_after = length(&lower, s.size).or(scope.para.space_after),
            "line-height" => {
                scope.para.line_height = if let Some(p) = lower.strip_suffix('%') {
                    p.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).map(|v| LineHeight::Factor(v / 100.0))
                } else if let Ok(v) = lower.parse::<f64>() {
                    (v.is_finite() && v > 0.0).then_some(LineHeight::Factor(v))
                } else {
                    length(&lower, s.size).filter(|v| *v > 0.0).map(LineHeight::Points)
                }
                .or(scope.para.line_height)
            }
            _ => {}
        }
    }
}

/// The first family of a `font-family` list, unquoted; `None` for generic or empty names.
fn family(value: &str) -> Option<String> {
    let first = value.split(',').next()?.trim().trim_matches(['\'', '"']).trim();
    let name: String = first.chars().take(64).collect();
    (!name.is_empty()).then_some(name)
}

/// A CSS length in points (`pt`, `px` as points, `in`, `mm`, `cm`, `em` of `em_of`, or a bare number).
fn length(value: &str, em_of: Option<f64>) -> Option<f64> {
    let v = value.trim();
    let (number, unit) =
        v.find(|c: char| c.is_ascii_alphabetic() || c == '%').map_or((v, ""), |i| (v.get(..i).unwrap_or(v), v.get(i..).unwrap_or("")));
    let n: f64 = number.trim().trim_start_matches('+').parse().ok()?;
    let pt = match unit.trim() {
        "" | "pt" | "px" => n,
        "in" => n * 72.0,
        "mm" => n * 72.0 / 25.4,
        "cm" => n * 72.0 / 2.54,
        "pc" => n * 12.0,
        "em" => n * em_of.unwrap_or(12.0),
        "%" => n / 100.0 * em_of.unwrap_or(12.0),
        _ => return None,
    };
    (pt.is_finite() && pt.abs() <= 14_400.0).then_some(pt)
}

fn weight(value: &str) -> Option<bool> {
    match value {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        n => n.parse::<u32>().ok().map(|w| w >= 600),
    }
}

fn italic(value: &str) -> Option<bool> {
    match value {
        "italic" | "oblique" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

/// `font-stretch` as a horizontal scale in percent.
fn stretch(value: &str) -> Option<f64> {
    if let Some(p) = value.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).map(|v| v.clamp(1.0, 1000.0));
    }
    Some(match value {
        "ultra-condensed" => 50.0,
        "extra-condensed" => 62.5,
        "condensed" => 75.0,
        "semi-condensed" => 87.5,
        "normal" => return None,
        "semi-expanded" => 112.5,
        "expanded" => 125.0,
        "extra-expanded" => 150.0,
        "ultra-expanded" => 200.0,
        _ => return None,
    })
}

/// The `font` shorthand: optional style and weight, a size (`12pt` or `12pt/14pt`), then families.
fn shorthand(value: &str, s: &mut RunStyle) {
    let mut words = value.split_whitespace().peekable();
    while let Some(w) = words.peek() {
        let lower = w.to_ascii_lowercase();
        if let Some(i) = italic(&lower) {
            s.italic = Some(i);
        } else if let Some(b) = weight(&lower) {
            s.bold = Some(b);
        } else if lower == "small-caps" {
        } else if let Some(size) = length(lower.split('/').next().unwrap_or(&lower), s.size) {
            s.size = Some(size);
            words.next();
            break;
        } else {
            break;
        }
        words.next();
    }
    let rest: Vec<&str> = words.collect();
    if let Some(f) = family(&rest.join(" ")) {
        s.family = Some(f);
    }
}

/// A CSS colour: `#rgb`, `#rrggbb`, `rgb(r, g, b)` (0–255 or percentages) or a basic name.
fn color(value: &str) -> Option<[f64; 3]> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        let digits: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
        return match digits.as_slice() {
            [r, g, b] => Some([*r as f64 / 15.0, *g as f64 / 15.0, *b as f64 / 15.0]),
            [r1, r2, g1, g2, b1, b2] => Some([(r1 * 16 + r2) as f64 / 255.0, (g1 * 16 + g2) as f64 / 255.0, (b1 * 16 + b2) as f64 / 255.0]),
            _ => None,
        };
    }
    if let Some(args) = v.strip_prefix("rgb(").and_then(|a| a.strip_suffix(')')) {
        let parts: Vec<f64> = args
            .split(',')
            .map(|p| {
                let p = p.trim();
                match p.strip_suffix('%') {
                    Some(pc) => pc.trim().parse::<f64>().ok().map(|x| x / 100.0),
                    None => p.parse::<f64>().ok().map(|x| x / 255.0),
                }
            })
            .collect::<Option<_>>()?;
        return match parts.as_slice() {
            [r, g, b] if [r, g, b].iter().all(|x| x.is_finite()) => Some([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]),
            _ => None,
        };
    }
    Some(match v {
        "black" => [0.0, 0.0, 0.0],
        "white" => [1.0, 1.0, 1.0],
        "red" => [1.0, 0.0, 0.0],
        "green" => [0.0, 0.5, 0.0],
        "lime" => [0.0, 1.0, 0.0],
        "blue" => [0.0, 0.0, 1.0],
        "yellow" => [1.0, 1.0, 0.0],
        "gray" | "grey" => [0.5, 0.5, 0.5],
        "silver" => [0.75, 0.75, 0.75],
        "maroon" => [0.5, 0.0, 0.0],
        "navy" => [0.0, 0.0, 0.5],
        "purple" => [0.5, 0.0, 0.5],
        "teal" => [0.0, 0.5, 0.5],
        "olive" => [0.5, 0.5, 0.0],
        "orange" => [1.0, 0.647, 0.0],
        _ => return None,
    })
}
