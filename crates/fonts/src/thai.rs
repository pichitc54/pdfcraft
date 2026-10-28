//! The Thai document face (Sarabun), found at runtime first, for field appearances (ADR 0001):
//! shaping, whole-font embedding as a Type 0 font, Thai word wrapping and line breaks, and the
//! สระอำ repair Edit text reads Word's Thai with.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::collections::BTreeMap;

use pdfcraft_cos::{Dict, Document, ObjRef, Object, PdfString, Stream};
use skrifa::instance::{LocationRef, Size};
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// The family drawn into field appearances.
const DOCUMENT_FAMILY: &str = "Sarabun";
/// Larger files are not read: a font path is still untrusted input.
const MAX_BYTES: u64 = 32 << 20;
/// Faces tried in a collection (`.ttc`).
const MAX_FACES: u32 = 16;
/// Files looked at per folder, so a huge fonts folder can't stall start-up.
const MAX_FILES: usize = 4096;
/// Name-table strings longer than this are cut (they are untrusted).
const MAX_NAME: usize = 64;
/// Font resources looked at when searching for an embedded face.
const MAX_RESOURCE_FONTS: usize = 256;
/// Thai character ko kai: a face must map it to count as Thai.
const PROBE: char = '\u{0e01}';

/// The environment variable naming a folder searched before the installed fonts.
pub const DIR_VAR: &str = "PDFCRAFT_THAI_FONT_DIR";

/// The folders searched for Thai faces, best first: `env_dir` (the [`DIR_VAR`] value), the user's
/// fonts (`~/Library/Fonts`, where Font Book installs), then `/Library/Fonts`. Missing or empty
/// inputs are left out.
pub fn search_dirs(env_dir: Option<&OsStr>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env_dir.filter(|d| !d.is_empty()).map(PathBuf::from).into_iter().collect();
    dirs.extend(home.filter(|h| !h.as_os_str().is_empty()).map(|h| h.join("Library/Fonts")));
    dirs.push(PathBuf::from("/Library/Fonts"));
    dirs
}

/// One style of a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// Where a face was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    /// The optional craft-fonts build input.
    CraftFonts,
}

/// A Thai face: its bytes and which face of the file it is (`.ttc` collections hold several).
#[derive(Clone, Debug, PartialEq)]
pub struct ThaiFace {
    pub family: String,
    pub style: Style,
    pub bytes: Arc<[u8]>,
    pub index: u32,
    pub source: Source,
}

/// The Thai document face (Sarabun) in the styles found; Regular is always present.
#[derive(Clone, Debug)]
pub struct ThaiFaces {
    pub regular: ThaiFace,
    pub bold: Option<ThaiFace>,
    pub italic: Option<ThaiFace>,
    pub bold_italic: Option<ThaiFace>,
}

impl ThaiFaces {
    /// The face for a bold and/or italic field: that style when it was found, else Regular
    /// (never a synthesized weight or slant).
    pub fn for_style(&self, bold: bool, italic: bool) -> &ThaiFace {
        let found = match (bold, italic) {
            (true, true) => self.bold_italic.as_ref(),
            (true, false) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (false, false) => None,
        };
        found.unwrap_or(&self.regular)
    }
}

/// Sarabun from `dirs` (best folder first), else from craft-fonts. A set never mixes the two, so
/// its styles share one version.
pub fn find_document_faces(dirs: &[PathBuf]) -> Option<ThaiFaces> {
    document_faces_in(dirs).or_else(craft_document_faces)
}

/// The document faces the app found at start-up, for code that draws fields (which never
/// searches for fonts itself).
static INSTALLED: std::sync::OnceLock<Option<ThaiFaces>> = std::sync::OnceLock::new();

/// Make `faces` the document faces for this process. Only the first call counts; returns whether
/// this one did.
pub fn install_document_faces(faces: Option<ThaiFaces>) -> bool {
    INSTALLED.set(faces).is_ok()
}

/// Find the document faces in [`search_dirs`] (from [`DIR_VAR`] and `$HOME`) or craft-fonts and
/// install them. For an app's start-up; returns what is installed.
pub fn install_from_environment() -> Option<&'static ThaiFaces> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = search_dirs(std::env::var_os(DIR_VAR).as_deref(), home.as_deref());
    install_document_faces(find_document_faces(&dirs));
    installed_document_faces()
}

/// The faces [`install_document_faces`] installed (`None` if none, or not called).
pub fn installed_document_faces() -> Option<&'static ThaiFaces> {
    INSTALLED.get().and_then(Option::as_ref)
}

fn document_faces_in(dirs: &[PathBuf]) -> Option<ThaiFaces> {
    let mut slots: [Option<ThaiFace>; 4] = Default::default();
    'files: for path in font_files(dirs) {
        let Some(bytes) = read_font(&path) else { continue };
        for (family, style, index) in thai_faces(&bytes) {
            let Some(style) = style.filter(|_| family == DOCUMENT_FAMILY) else { continue };
            if let Some(slot @ None) = slots.get_mut(style as usize) {
                *slot = Some(ThaiFace { family, style, bytes: Arc::clone(&bytes), index, source: Source::File(path.clone()) });
            }
            if slots.iter().all(Option::is_some) {
                break 'files;
            }
        }
    }
    let [regular, bold, italic, bold_italic] = slots;
    Some(ThaiFaces { regular: regular?, bold, italic, bold_italic })
}

fn craft_document_faces() -> Option<ThaiFaces> {
    let mut slots: [Option<ThaiFace>; 4] = Default::default();
    for face in crate::CRAFT_FONTS.iter().filter(|f| f.covers("Thai") && f.family == DOCUMENT_FAMILY) {
        let Some(style) = style_named(face.style) else { continue };
        if let Some(slot @ None) = slots.get_mut(style as usize) {
            *slot = Some(ThaiFace { family: face.family.to_owned(), style, bytes: Arc::from(face.bytes), index: 0, source: Source::CraftFonts });
        }
    }
    let [regular, bold, italic, bold_italic] = slots;
    Some(ThaiFaces { regular: regular?, bold, italic, bold_italic })
}

/// The face's ascent and descent (negative) in em, from its own metrics.
pub fn vertical_metrics(face: &ThaiFace) -> Option<(f64, f64)> {
    let font = FontRef::from_index(&face.bytes, face.index).ok()?;
    let m = font.metrics(Size::unscaled(), LocationRef::default());
    let upem = f64::from(m.units_per_em);
    (upem > 0.0).then(|| (f64::from(m.ascent) / upem, f64::from(m.descent) / upem))
}

/// The face's x-height (the height of its lowercase Latin letters) in em, from its own metrics.
pub fn x_height(face: &ThaiFace) -> Option<f64> {
    let font = FontRef::from_index(&face.bytes, face.index).ok()?;
    let m = font.metrics(Size::unscaled(), LocationRef::default());
    let upem = f64::from(m.units_per_em);
    let x = f64::from(m.x_height?);
    (upem > 0.0 && x > 0.0).then(|| x / upem)
}

/// How far above the baseline Thai reaches in `face`, in em: the top of its tallest stacks (a tone
/// mark on an upper vowel, as in หมื่น), shaped as they are drawn.
pub fn mark_top(face: &ThaiFace) -> Option<f64> {
    let font = FontRef::from_index(&face.bytes, face.index).ok()?;
    let upem = f64::from(font.metrics(Size::unscaled(), LocationRef::default()).units_per_em);
    let glyphs = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let run = shape(face, "หมื่นปั้นปิ๊ฟี๋ที่สึ่งกั๊ก").ok()?;
    let top = run
        .glyphs
        .iter()
        .filter_map(|g| glyphs.bounds(GlyphId::new(u32::from(g.id))).map(|b| f64::from(b.y_max) / upem + g.dy))
        .fold(f64::NEG_INFINITY, f64::max);
    (upem > 0.0 && top.is_finite()).then_some(top)
}

/// Whether `face` has a glyph for every character of `text` (line breaks and other controls
/// aside), so drawing it never shows a missing-glyph box.
pub fn covers(face: &ThaiFace, text: &str) -> bool {
    let Ok(font) = FontRef::from_index(&face.bytes, face.index) else { return false };
    let charmap = font.charmap();
    text.chars().all(|c| c.is_control() || charmap.map(c).is_some())
}

/// Longest text shaped in one call.
pub const MAX_SHAPE_CHARS: usize = 4096;

/// One shaped glyph. Lengths are in em (font units / units per em).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub id: u16,
    /// The pen advance after shaping (marks: 0).
    pub advance: f64,
    /// The glyph's own advance in the font (`hmtx`), which is what `/W` holds.
    pub width: f64,
    pub dx: f64,
    pub dy: f64,
    /// Byte offset in the source text of the cluster this glyph belongs to.
    pub cluster: usize,
}

/// Glyphs in visual order (Thai is left to right).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeError {
    /// More than [`MAX_SHAPE_CHARS`] characters.
    TooLong,
    /// The face's bytes don't parse.
    BadFont,
}

/// Shape `text` with `face` (OpenType Thai shaping: mark positioning, contextual forms).
pub fn shape(face: &ThaiFace, text: &str) -> Result<ShapedRun, ShapeError> {
    if text.chars().nth(MAX_SHAPE_CHARS).is_some() {
        return Err(ShapeError::TooLong);
    }
    if text.is_empty() {
        return Ok(ShapedRun::default());
    }
    let font = FontRef::from_index(&face.bytes, face.index).map_err(|_| ShapeError::BadFont)?;
    let upem = f64::from(font.metrics(Size::unscaled(), LocationRef::default()).units_per_em);
    if upem <= 0.0 {
        return Err(ShapeError::BadFont);
    }
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let data = harfrust::ShaperData::new(&font);
    let shaper = data.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
    let glyphs = shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .map(|(info, pos)| ShapedGlyph {
            // Glyph ids of TrueType/CFF fonts fit in 16 bits; anything else becomes .notdef.
            id: u16::try_from(info.glyph_id).unwrap_or(0),
            advance: f64::from(pos.x_advance) / upem,
            width: f64::from(metrics.advance_width(GlyphId::new(info.glyph_id)).unwrap_or(0.0)) / upem,
            dx: f64::from(pos.x_offset) / upem,
            dy: f64::from(pos.y_offset) / upem,
            cluster: info.cluster as usize,
        })
        .collect();
    Ok(ShapedRun { glyphs })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbedError {
    /// The face's bytes don't parse.
    BadFont,
    /// `font` is not a font [`embed`] wrote.
    NotEmbedded,
}

/// Embed `face` whole into `doc` as a Type 0 font (CIDFontType2, Identity-H, ToUnicode).
/// Returns the font dictionary's reference. The font file is embedded unsubset, so other
/// viewers can redraw the field with it.
pub fn embed(doc: &mut Document, face: &ThaiFace) -> Result<ObjRef, EmbedError> {
    // FontFile2 holds one TrueType font, not a collection.
    if face.index != 0 || face.bytes.starts_with(b"ttcf") {
        return Err(EmbedError::BadFont);
    }
    let font = FontRef::new(&face.bytes).map_err(|_| EmbedError::BadFont)?;
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let upem = f64::from(metrics.units_per_em);
    if upem <= 0.0 {
        return Err(EmbedError::BadFont);
    }
    let em = |v: f32| Object::Real((f64::from(v) * 1000.0 / upem).round());
    let glyphs = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let count = glyphs.glyph_count().min(u32::from(u16::MAX) + 1);
    let widths: Vec<Object> = (0..count).map(|g| em(glyphs.advance_width(GlyphId::new(g)).unwrap_or(0.0))).collect();
    let base = postscript_name(&font, face);

    let mut file = Dict::new();
    file.set(b"Length1".to_vec(), Object::Int(face.bytes.len() as i64));
    let file = doc.add(Object::Stream(Stream::flate(file, &face.bytes)));
    let bbox = metrics.bounds.map_or([0.0, metrics.descent, metrics.units_per_em as f32, metrics.ascent], |b| [b.x_min, b.y_min, b.x_max, b.y_max]);
    let mut descriptor = Dict::new();
    descriptor.set(b"Type".to_vec(), Object::name("FontDescriptor"));
    descriptor.set(b"FontName".to_vec(), Object::name(&base));
    descriptor.set(b"FontFamily".to_vec(), Object::String(PdfString::literal(face.family.as_bytes().to_vec())));
    let italic = matches!(face.style, Style::Italic | Style::BoldItalic);
    // Symbolic (4): the font has glyphs outside the standard Latin set; Italic (64).
    descriptor.set(b"Flags".to_vec(), Object::Int(if italic { 4 | 64 } else { 4 }));
    descriptor.set(b"FontBBox".to_vec(), Object::Array(bbox.into_iter().map(em).collect()));
    descriptor.set(b"ItalicAngle".to_vec(), Object::Real(f64::from(metrics.italic_angle)));
    descriptor.set(b"Ascent".to_vec(), em(metrics.ascent));
    descriptor.set(b"Descent".to_vec(), em(metrics.descent));
    descriptor.set(b"CapHeight".to_vec(), em(metrics.cap_height.unwrap_or(metrics.ascent)));
    // Required but unused by viewers for embedded fonts; a typical regular/bold stem.
    descriptor.set(b"StemV".to_vec(), Object::Int(if matches!(face.style, Style::Bold | Style::BoldItalic) { 140 } else { 80 }));
    descriptor.set(b"FontFile2".to_vec(), Object::Ref(file));
    let descriptor = doc.add(Object::Dict(descriptor));

    let mut info = Dict::new();
    info.set(b"Registry".to_vec(), Object::String(PdfString::literal(b"Adobe".to_vec())));
    info.set(b"Ordering".to_vec(), Object::String(PdfString::literal(b"Identity".to_vec())));
    info.set(b"Supplement".to_vec(), Object::Int(0));
    let mut cid = Dict::new();
    cid.set(b"Type".to_vec(), Object::name("Font"));
    cid.set(b"Subtype".to_vec(), Object::name("CIDFontType2"));
    cid.set(b"BaseFont".to_vec(), Object::name(&base));
    cid.set(b"CIDSystemInfo".to_vec(), Object::Dict(info));
    cid.set(b"FontDescriptor".to_vec(), Object::Ref(descriptor));
    cid.set(b"CIDToGIDMap".to_vec(), Object::name("Identity"));
    cid.set(b"W".to_vec(), Object::Array(vec![Object::Int(0), Object::Array(widths)]));
    let cid = doc.add(Object::Dict(cid));

    let mut unicode = BTreeMap::new();
    for (ch, gid) in font.charmap().mappings() {
        let (Some(c), Ok(gid)) = (char::from_u32(ch), u16::try_from(gid.to_u32())) else { continue };
        if gid != 0 && !c.is_control() {
            unicode.entry(gid).or_insert_with(|| c.to_string());
        }
    }
    let to_unicode = doc.add(Object::Stream(to_unicode_cmap(&unicode)));
    let mut type0 = Dict::new();
    type0.set(b"Type".to_vec(), Object::name("Font"));
    type0.set(b"Subtype".to_vec(), Object::name("Type0"));
    type0.set(b"BaseFont".to_vec(), Object::name(&base));
    type0.set(b"Encoding".to_vec(), Object::name("Identity-H"));
    type0.set(b"DescendantFonts".to_vec(), Object::Array(vec![Object::Ref(cid)]));
    type0.set(b"ToUnicode".to_vec(), Object::Ref(to_unicode));
    Ok(doc.add(Object::Dict(type0)))
}

/// The face's PostScript name (letters, digits and `-` only), else `Family-Style`.
fn postscript_name(font: &FontRef, face: &ThaiFace) -> String {
    let clean = |s: String| s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(MAX_NAME).collect::<String>();
    font.localized_strings(StringId::POSTSCRIPT_NAME)
        .english_or_first()
        .map(|s| clean(s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| clean(format!("{}-{:?}", face.family, face.style)))
}

/// A ToUnicode CMap (bfchar blocks of at most 100 entries, §9.10.3) for two-byte codes.
fn to_unicode_cmap(map: &BTreeMap<u16, String>) -> Stream {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<_> = map.iter().collect();
    for block in entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", block.len()));
        for (gid, text) in block {
            let hex: String = text.encode_utf16().map(|u| format!("{u:04X}")).collect();
            out.push_str(&format!("<{gid:04X}> <{hex}>\n"));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    Stream::flate(Dict::new(), out.as_bytes())
}

/// Record in `font`'s ToUnicode what each glyph of `run` (shaped from `text`) stands for, so
/// alternate forms (ญ without its descender, shifted tone marks) copy as their characters.
/// Glyphs that already have an entry keep it. A cluster whose glyphs don't pair up one-to-one
/// with its characters (สระอำ: two glyphs for one character) adds nothing; [`show`] marks it
/// with `/ActualText` instead.
pub fn add_unicode(doc: &mut Document, font: ObjRef, run: &ShapedRun, text: &str) -> Result<(), EmbedError> {
    let dict = doc.get(font).as_dict().cloned().ok_or(EmbedError::NotEmbedded)?;
    let Some(Object::Ref(cmap_ref)) = dict.get(b"ToUnicode").cloned().filter(|_| dict.name(b"Subtype") == Some(b"Type0")) else {
        return Err(EmbedError::NotEmbedded);
    };
    if !matches!(&*doc.get(cmap_ref), Object::Stream(_)) {
        return Err(EmbedError::NotEmbedded);
    }
    // The entries already there, read the way text extraction reads them.
    let metrics = crate::pdf::Metrics::from_dict(doc, &dict);
    let mut map: BTreeMap<u16, String> = (0..=u16::MAX).filter_map(|code| Some((code, metrics.text_of(u32::from(code))?.into_owned()))).collect();
    let before = map.len();
    for (glyphs, chars) in clusters(run, text) {
        if glyphs.len() != chars.len() {
            continue;
        }
        for (g, c) in glyphs.iter().zip(chars) {
            if g.id != 0 {
                map.entry(g.id).or_insert_with(|| c.to_string());
            }
        }
    }
    if map.len() != before {
        doc.set(cmap_ref, Object::Stream(to_unicode_cmap(&map)));
    }
    Ok(())
}

/// The run's clusters in order: each one's glyphs and the characters of its source text.
fn clusters<'a>(run: &'a ShapedRun, text: &'a str) -> Vec<(&'a [ShapedGlyph], Vec<char>)> {
    let mut out = Vec::new();
    let mut rest = run.glyphs.as_slice();
    while let Some(first) = rest.first() {
        let n = rest.iter().take_while(|g| g.cluster == first.cluster).count();
        let (glyphs, tail) = rest.split_at(n);
        let end = tail.iter().map(|g| g.cluster).find(|&c| c > first.cluster).unwrap_or(text.len());
        let chars = text.get(first.cluster..end).map(|s| s.chars().collect()).unwrap_or_default();
        out.push((glyphs, chars));
        rest = tail;
    }
    out
}

/// The entry of the font resources `fonts` (e.g. the form's `/DR /Font`) that is `face` as
/// [`embed`] wrote it: same `/BaseFont` and a font file identical to the face's bytes. Returns
/// the resource name and the font's reference.
pub fn find_embedded(doc: &Document, fonts: &Dict, face: &ThaiFace) -> Option<(Vec<u8>, ObjRef)> {
    let base = FontRef::from_index(&face.bytes, face.index).ok().map(|font| postscript_name(&font, face))?;
    fonts.iter().take(MAX_RESOURCE_FONTS).find_map(|(name, obj)| {
        let Object::Ref(r) = obj else { return None };
        let font = doc.get(*r).as_dict().cloned()?;
        is_embedding_of(doc, &font, face, &base).then(|| (name.clone(), *r))
    })
}

/// The installed document face that the font dictionary `font` is an embedding of, as [`embed`]
/// wrote it (see [`find_embedded`]). `None` for any other font, or with no faces installed.
pub fn embedded_face(doc: &Document, font: &Dict) -> Option<&'static ThaiFace> {
    // Cheap checks first: this runs for every field font a form's appearances look up.
    if font.name(b"Subtype") != Some(b"Type0") || !font.name(b"BaseFont").is_some_and(|b| b.starts_with(DOCUMENT_FAMILY.as_bytes())) {
        return None;
    }
    let faces = installed_document_faces()?;
    [Some(&faces.regular), faces.bold.as_ref(), faces.italic.as_ref(), faces.bold_italic.as_ref()]
        .into_iter()
        .flatten()
        .find(|face| FontRef::from_index(&face.bytes, face.index).is_ok_and(|f| is_embedding_of(doc, font, face, &postscript_name(&f, face))))
}

/// Whether `font` is `face` as [`embed`] wrote it: Type 0 with the face's PostScript name `base`
/// and a font file identical to the face's bytes.
fn is_embedding_of(doc: &Document, font: &Dict, face: &ThaiFace, base: &str) -> bool {
    if font.name(b"Subtype") != Some(b"Type0") || font.name(b"BaseFont") != Some(base.as_bytes()) {
        return false;
    }
    let file = (|| {
        let cid = font.get(b"DescendantFonts").and_then(|d| doc.resolve(d).as_array().and_then(|a| a.first().cloned()))?;
        let descriptor = doc.dict(&cid)?.get(b"FontDescriptor").cloned()?;
        doc.dict(&descriptor)?.get(b"FontFile2").cloned()
    })();
    let Some(file) = file else { return false };
    let Object::Stream(file) = &*doc.resolve(&file) else { return false };
    // The declared length rules most fonts out before decompressing.
    if file.dict.get(b"Length1").and_then(|o| doc.resolve(o).as_int()).is_some_and(|n| usize::try_from(n).ok() != Some(face.bytes.len())) {
        return false;
    }
    file.decoded_within(MAX_BYTES as usize).is_ok_and(|bytes| bytes.as_slice() == &*face.bytes)
}

/// Content operators that draw `run` (shaped from `text`) at font size `size`, to follow `Tf`
/// inside `BT … ET`: each glyph lands at its shaped position (`TJ` adjustments for x, text rise
/// for y), and a cluster whose glyphs can't spell its text through ToUnicode (สระอำ) is wrapped
/// in a `/Span` with `/ActualText`.
pub fn show(run: &ShapedRun, text: &str, size: f64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut items: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, items: &mut Vec<u8>| {
        if !items.is_empty() {
            out.push(b'[');
            out.append(items);
            out.extend_from_slice(b"] TJ\n");
        }
    };
    // Positions in em from the start: where the shaper's pen is, and where the PDF's is.
    let (mut pen, mut at, mut rise) = (0.0, 0.0, 0.0);
    for (glyphs, chars) in clusters(run, text) {
        let span = glyphs.len() != chars.len();
        if span {
            flush(&mut out, &mut items);
            let mut props = Dict::new();
            props.set(b"ActualText".to_vec(), Object::String(PdfString::text(&chars.iter().collect::<String>())));
            out.extend_from_slice(b"/Span ");
            pdfcraft_cos::serialize(&Object::Dict(props), &mut out);
            out.extend_from_slice(b" BDC\n");
        }
        for g in glyphs {
            let y = g.dy * size;
            if (y - rise).abs() > 1e-6 {
                flush(&mut out, &mut items);
                out.extend_from_slice(format!("{} Ts\n", num(y)).as_bytes());
                rise = y;
            }
            let target = pen + g.dx;
            if (target - at).abs() > 1e-6 {
                // TJ numbers move left by thousandths of an em.
                items.extend_from_slice(format!("{} ", num((at - target) * 1000.0)).as_bytes());
            }
            items.extend_from_slice(format!("<{:04X}>", g.id).as_bytes());
            at = target + g.width;
            pen += g.advance;
        }
        if span {
            flush(&mut out, &mut items);
            out.extend_from_slice(b"EMC\n");
        }
    }
    flush(&mut out, &mut items);
    if rise != 0.0 {
        out.extend_from_slice(b"0 Ts\n");
    }
    out
}

/// A number for a content stream: at most three decimals, no trailing zeros.
/// Non-finite values (from an untrusted font size) become 0.
fn num(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// The Identity-H codes (two bytes per glyph id) of `run`.
pub fn encode(run: &ShapedRun) -> Vec<u8> {
    run.glyphs.iter().flat_map(|g| g.id.to_be_bytes()).collect()
}

/// Break `text` into lines no wider than `width` points at font size `size`: paragraphs on
/// newlines, then greedily at line-break opportunities (UAX #14, with dictionary word
/// boundaries for Thai). A word wider than a line is split between grapheme clusters.
pub fn wrap(face: &ThaiFace, text: &str, size: f64, width: f64) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation as _;
    let segmenter = icu_segmenter::LineSegmenter::new_dictionary(Default::default());
    // Lines are only as long as a field is wide, so shaping each candidate stays cheap.
    let fits = |s: &str| shape(face, s.trim_end()).map_or(0.0, |r| r.glyphs.iter().map(|g| g.advance).sum::<f64>()) * size <= width;
    let mut lines = Vec::new();
    for para in text.split('\n').map(|p| p.trim_end_matches('\r')) {
        let mut line = String::new();
        let breaks: Vec<usize> = segmenter.segment_str(para).collect();
        for piece in breaks.windows(2).filter_map(|b| para.get(b[0]..b[1])) {
            if !line.is_empty() && !fits(&format!("{line}{piece}")) {
                lines.push(std::mem::take(&mut line).trim_end().to_owned());
            }
            line.push_str(piece);
            // A word wider than a whole line: split it between grapheme clusters.
            while !fits(&line) && line.trim_end().graphemes(true).nth(1).is_some() {
                let mut head = String::new();
                for g in line.graphemes(true) {
                    if !head.is_empty() && !fits(&format!("{head}{g}")) {
                        break;
                    }
                    head.push_str(g);
                }
                let rest = line.get(head.len()..).unwrap_or_default().to_owned();
                lines.push(head.trim_end().to_owned());
                line = rest;
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    lines
}

/// `text` with สระอำ restored where a producer (Word) stored its nikhahit as a space: a Thai
/// character, a space and sara aa read as sara am. Sara aa never starts a Thai word, so a space
/// before it can only be that glyph.
pub fn repair_sara_am(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(" \u{0e32}") {
        return std::borrow::Cow::Borrowed(text);
    }
    let thai = |c: char| ('\u{0e01}'..='\u{0e4e}').contains(&c);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == ' ' && chars.get(i + 1) == Some(&'\u{0e32}') && i > 0 && chars.get(i - 1).is_some_and(|p| thai(*p)) {
            out.push('\u{0e33}');
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Where `text` may break between lines (UAX #14, with dictionary word boundaries for Thai), as
/// character indices, excluding the start.
pub fn line_breaks(text: &str) -> Vec<usize> {
    let segmenter = icu_segmenter::LineSegmenter::new_dictionary(Default::default());
    let breaks: std::collections::HashSet<usize> = segmenter.segment_str(text).collect();
    // Byte offsets to character indices.
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .enumerate()
        .filter(|&(i, byte)| i > 0 && breaks.contains(&byte))
        .map(|(i, _)| i)
        .collect()
}

/// The font files in `dirs`, folder by folder in the given order and by name within a folder.
pub(crate) fn font_files(dirs: &[PathBuf]) -> impl Iterator<Item = PathBuf> + '_ {
    dirs.iter().flat_map(|dir| {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(OsStr::to_str).is_some_and(|e| ["ttf", "otf", "ttc"].contains(&e.to_ascii_lowercase().as_str())))
            .take(MAX_FILES)
            .collect();
        files.sort();
        files
    })
}

/// The file's bytes, if it is a regular file no larger than [`MAX_BYTES`].
pub(crate) fn read_font(path: &Path) -> Option<Arc<[u8]>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    std::fs::read(path).ok().map(Arc::from)
}

/// Family, style (when it is one of the four [`Style`]s) and index of every face in the file that
/// parses and maps [`PROBE`].
fn thai_faces(bytes: &[u8]) -> Vec<(String, Option<Style>, u32)> {
    (0..MAX_FACES)
        .map_while(|index| FontRef::from_index(bytes, index).ok().map(|font| (font, index)))
        .filter(|(font, _)| font.charmap().map(PROBE).is_some())
        .filter_map(|(font, index)| {
            let family = name(&font, StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME)?;
            let style = name(&font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME).and_then(|s| style_named(&s));
            Some((family, style, index))
        })
        .collect()
}

/// The typographic name (IDs 16/17), else the legacy one (IDs 1/2), cut to [`MAX_NAME`].
pub(crate) fn name(font: &FontRef, typographic: StringId, legacy: StringId) -> Option<String> {
    let s = font.localized_strings(typographic).english_or_first().or_else(|| font.localized_strings(legacy).english_or_first())?;
    Some(s.chars().take(MAX_NAME).collect())
}

pub(crate) fn style_named(subfamily: &str) -> Option<Style> {
    match subfamily.trim().to_ascii_lowercase().as_str() {
        "regular" | "normal" | "book" => Some(Style::Regular),
        "bold" => Some(Style::Bold),
        "italic" | "oblique" => Some(Style::Italic),
        "bold italic" | "bolditalic" | "bold oblique" => Some(Style::BoldItalic),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_dirs_put_the_env_folder_first_then_user_then_system() {
        let dirs = search_dirs(Some(OsStr::new("/tmp/thai")), Some(Path::new("/Users/a")));
        assert_eq!(dirs, [PathBuf::from("/tmp/thai"), PathBuf::from("/Users/a/Library/Fonts"), PathBuf::from("/Library/Fonts")]);
    }

    #[test]
    fn search_dirs_skip_a_missing_or_empty_env_folder_and_home() {
        assert_eq!(search_dirs(None, None), [PathBuf::from("/Library/Fonts")]);
        assert_eq!(search_dirs(Some(OsStr::new("")), Some(Path::new(""))), [PathBuf::from("/Library/Fonts")]);
    }

    /// The Sarabun folder from `PDFCRAFT_THAI_FONT_DIR`, or `None` (and the test is skipped).
    fn sarabun_dir(test: &str) -> Option<PathBuf> {
        let dir = std::env::var_os(DIR_VAR).map(PathBuf::from);
        if dir.is_none() {
            eprintln!("skipping {test}: set {DIR_VAR} to a folder with Sarabun to run it");
        }
        dir
    }

    #[test]
    fn sarabun_styles_are_found_by_their_names() {
        let Some(dir) = sarabun_dir("sarabun_styles_are_found_by_their_names") else { return };
        let faces = find_document_faces(&[dir]).expect("Sarabun Regular");
        assert_eq!((faces.regular.family.as_str(), faces.regular.style), ("Sarabun", Style::Regular));
        assert_eq!(faces.bold.map(|f| f.style), Some(Style::Bold));
        assert_eq!(faces.italic.map(|f| f.style), Some(Style::Italic));
        assert_eq!(faces.bold_italic.map(|f| f.style), Some(Style::BoldItalic));
    }

    /// A fresh, empty folder under the system temp folder.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-thai-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn folders_without_a_usable_sarabun_give_none() {
        let dir = temp_dir("unusable");
        std::fs::write(dir.join("Sarabun-Regular.ttf"), b"not a font at all").unwrap();
        // Inter parses but has no Thai.
        std::fs::write(dir.join("Inter.ttf"), include_bytes!("../../../assets/fonts/Inter-Regular.ttf")).unwrap();
        // Over the size cap: never read.
        std::fs::File::create(dir.join("huge.ttf")).unwrap().set_len(MAX_BYTES + 1).unwrap();
        // A folder is not a font.
        std::fs::create_dir(dir.join("folder.ttf")).unwrap();
        // Nothing comes from these folders (a craft-fonts build may still supply Sarabun).
        let found = find_document_faces(&[dir.clone(), dir.join("missing")]);
        assert!(found.is_none_or(|f| f.regular.source == Source::CraftFonts));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_thai_face_of_another_family_is_not_sarabun() {
        let Some(sarabun) = sarabun_dir("a_thai_face_of_another_family_is_not_sarabun") else { return };
        // Thonburi ships with macOS: Thai, but not Sarabun.
        let thonburi = Path::new("/System/Library/Fonts/Supplemental/Thonburi.ttc");
        if !thonburi.is_file() {
            return;
        }
        let dir = temp_dir("other-family");
        std::fs::copy(thonburi, dir.join("Sarabun-Regular.ttc")).unwrap();
        let found = find_document_faces(std::slice::from_ref(&dir));
        assert!(found.is_none_or(|f| f.regular.source == Source::CraftFonts));
        // The real one in a later folder is still found.
        assert!(find_document_faces(&[dir.clone(), sarabun]).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_sarabun_file_craft_fonts_supplies_it() {
        if !crate::CRAFT_FONTS.iter().any(|f| f.family == DOCUMENT_FAMILY) {
            eprintln!("skipping without_a_sarabun_file_craft_fonts_supplies_it: build with CRAFT_FONTS_DIR holding Sarabun to run it");
            return;
        }
        let faces = find_document_faces(&[]).expect("Sarabun from craft-fonts");
        assert_eq!((faces.regular.source, faces.regular.style), (Source::CraftFonts, Style::Regular));
        assert_eq!(faces.bold.map(|f| f.style), Some(Style::Bold));
        assert_eq!(faces.bold_italic.map(|f| f.style), Some(Style::BoldItalic));
    }

    /// Sarabun Regular from `PDFCRAFT_THAI_FONT_DIR` or craft-fonts, or `None` (test skipped).
    fn sarabun(test: &str) -> Option<ThaiFace> {
        let dirs: Vec<PathBuf> = std::env::var_os(DIR_VAR).map(PathBuf::from).into_iter().collect();
        let face = find_document_faces(&dirs).map(|f| f.regular);
        if face.is_none() {
            eprintln!("skipping {test}: set {DIR_VAR} to a folder with Sarabun (or build with craft-fonts) to run it");
        }
        face
    }

    /// The glyph shaped for the character at byte `at` of `text` that isn't the cluster's base.
    fn mark(run: &ShapedRun, text: &str, ch: char) -> ShapedGlyph {
        let at = text.find(ch).unwrap();
        let cluster = run.glyphs.iter().filter(|g| g.cluster <= at).map(|g| g.cluster).max().unwrap();
        *run.glyphs.iter().rfind(|g| g.cluster == cluster).unwrap()
    }

    #[test]
    fn a_tone_mark_over_an_upper_vowel_sits_higher() {
        let Some(face) = sarabun("a_tone_mark_over_an_upper_vowel_sits_higher") else { return };
        let alone = shape(&face, "ท่").unwrap();
        let stacked = shape(&face, "ที่").unwrap();
        let (a, s) = (mark(&alone, "ท่", '่'), mark(&stacked, "ที่", '่'));
        assert!(s.id != a.id || s.dy > a.dy + 0.05, "alone {a:?}, over sara ii {s:?}");
    }

    #[test]
    fn yo_ying_drops_its_descender_before_a_lower_vowel() {
        let Some(face) = sarabun("yo_ying_drops_its_descender_before_a_lower_vowel") else { return };
        let alone = shape(&face, "ญ").unwrap().glyphs[0];
        let with_u = shape(&face, "ญุ").unwrap().glyphs[0];
        assert_ne!(alone.id, with_u.id);
    }

    #[test]
    fn a_tone_mark_on_a_tall_consonant_moves_left() {
        let Some(face) = sarabun("a_tone_mark_on_a_tall_consonant_moves_left") else { return };
        let short = mark(&shape(&face, "ก่").unwrap(), "ก่", '่');
        let tall = mark(&shape(&face, "ป่").unwrap(), "ป่", '่');
        assert!(tall.id != short.id || tall.dx < short.dx - 0.02, "on ko kai {short:?}, on po pla {tall:?}");
    }

    #[test]
    fn sara_am_becomes_nikhahit_and_sara_aa() {
        let Some(face) = sarabun("sara_am_becomes_nikhahit_and_sara_aa") else { return };
        let run = shape(&face, "กำ").unwrap();
        assert_eq!(run.glyphs.len(), 3, "{run:?}");
    }

    #[test]
    fn marks_take_no_width() {
        let Some(face) = sarabun("marks_take_no_width") else { return };
        let width = |t: &str| shape(&face, t).unwrap().glyphs.iter().map(|g| g.advance).sum::<f64>();
        assert!(width("ท") > 0.3);
        assert!((width("ที่") - width("ท")).abs() < 1e-9, "{} vs {}", width("ที่"), width("ท"));
    }

    #[test]
    fn empty_and_overlong_text() {
        let Some(face) = sarabun("empty_and_overlong_text") else { return };
        assert_eq!(shape(&face, ""), Ok(ShapedRun::default()));
        assert_eq!(shape(&face, &"ก".repeat(MAX_SHAPE_CHARS)).map(|r| r.glyphs.len()), Ok(MAX_SHAPE_CHARS));
        assert_eq!(shape(&face, &"ก".repeat(MAX_SHAPE_CHARS + 1)), Err(ShapeError::TooLong));
    }

    #[test]
    fn embedded_sarabun_reads_back_as_the_text() {
        let Some(face) = sarabun("embedded_sarabun_reads_back_as_the_text") else { return };
        let text = "สวัสดี ที่ไม่ ญุ ป่า Smith";
        let run = shape(&face, text).unwrap();
        let mut doc = Document::new_empty();
        let font = embed(&mut doc, &face).unwrap();
        add_unicode(&mut doc, font, &run, text).unwrap();
        let dict = doc.get(font).as_dict().cloned().unwrap();
        let metrics = crate::pdf::Metrics::from_dict(&doc, &dict);
        let bytes = encode(&run);
        assert_eq!(metrics.decode(&bytes), text);
        let codes = metrics.codes(&bytes);
        assert_eq!(codes.len(), run.glyphs.len());
        for (g, (code, _)) in run.glyphs.iter().zip(codes) {
            if g.advance > 0.0 {
                assert!((metrics.width(code) - g.advance).abs() < 1e-3, "glyph {}: {} vs {}", g.id, metrics.width(code), g.advance);
            }
        }
    }

    #[test]
    fn an_embedded_face_is_found_again_after_save_and_reopen() {
        let dirs: Vec<PathBuf> = std::env::var_os(DIR_VAR).map(PathBuf::from).into_iter().collect();
        let Some(faces) = find_document_faces(&dirs) else { return };
        let Some(bold) = faces.bold.clone() else { return };
        let mut doc = Document::new_empty();
        let regular = embed(&mut doc, &faces.regular).unwrap();
        let mut fonts = Dict::new();
        fonts.set(b"Helv".to_vec(), Object::name("Helvetica"));
        fonts.set(b"Sarabun".to_vec(), Object::Ref(regular));
        let mut dr = Dict::new();
        dr.set(b"Font".to_vec(), Object::Dict(fonts));
        let mut acroform = Dict::new();
        acroform.set(b"DR".to_vec(), Object::Dict(dr));
        let root = doc.root().unwrap();
        doc.update_dict(root, |d| d.set(b"AcroForm".to_vec(), Object::Dict(acroform))).unwrap();
        let bytes = pdfcraft_cos::write_full(&doc, &pdfcraft_cos::SaveOptions::default()).unwrap();
        let doc = Document::open(std::sync::Arc::new(bytes)).unwrap();
        let catalog = doc.get(doc.root().unwrap());
        let dr = catalog.as_dict().and_then(|c| c.get(b"AcroForm")).and_then(|a| doc.dict(a)).and_then(|a| a.get(b"DR").cloned()).unwrap();
        let fonts = doc.dict(&dr).and_then(|d| d.get(b"Font").cloned()).and_then(|f| doc.dict(&f)).unwrap();
        let Some(Object::Ref(saved)) = fonts.get(b"Sarabun").cloned() else { panic!("Sarabun is in /DR") };
        assert_eq!(find_embedded(&doc, &fonts, &faces.regular), Some((b"Sarabun".to_vec(), saved)));
        assert_eq!(find_embedded(&doc, &fonts, &bold), None);
        assert_eq!(find_embedded(&doc, &Dict::new(), &faces.regular), None);
    }

    /// Where a PDF viewer draws each glyph of `ops` (ISO 32000-2 §9.4.4), in em from the text
    /// start, and the `/ActualText` of every marked span. Handles what [`show`] writes.
    fn replay(ops: &[u8], metrics: &crate::pdf::Metrics, size: f64) -> (Vec<(u16, f64, f64)>, Vec<String>) {
        use pdfcraft_cos::Lexer;
        let (mut x, mut rise) = (0.0, 0.0);
        let (mut glyphs, mut spans, mut operands) = (Vec::new(), Vec::new(), Vec::<Object>::new());
        let mut draw = |bytes: &[u8], x: &mut f64, rise: f64| {
            for (code, _) in metrics.codes(bytes) {
                glyphs.push((code as u16, *x / size, rise / size));
                *x += metrics.width(code) * size;
            }
        };
        let mut lexer = Lexer::new(ops, 0);
        loop {
            lexer.skip_ws();
            let Some(&b) = ops.get(lexer.pos) else { break };
            if b.is_ascii_alphabetic() || b == b'\'' || b == b'"' {
                match lexer.token() {
                    b"Ts" => rise = operands[0].as_f64().unwrap(),
                    b"Tj" => draw(&operands[0].as_string().unwrap().bytes, &mut x, rise),
                    b"TJ" => {
                        for item in operands[0].as_array().unwrap() {
                            match item {
                                Object::String(s) => draw(&s.bytes, &mut x, rise),
                                n => x -= n.as_f64().unwrap() / 1000.0 * size,
                            }
                        }
                    }
                    b"BDC" => {
                        let props = operands[1].as_dict().unwrap();
                        spans.push(props.get(b"ActualText").and_then(Object::as_string).unwrap().to_text());
                    }
                    b"EMC" => {}
                    op => panic!("unexpected operator {}", String::from_utf8_lossy(op)),
                }
                operands.clear();
            } else {
                operands.push(lexer.object().unwrap());
            }
        }
        (glyphs, spans)
    }

    /// Embeds `face`, shows `text` and replays it.
    fn shown(face: &ThaiFace, text: &str) -> (ShapedRun, Vec<(u16, f64, f64)>, Vec<String>) {
        let run = shape(face, text).unwrap();
        let mut doc = Document::new_empty();
        let font = embed(&mut doc, face).unwrap();
        let metrics = crate::pdf::Metrics::from_dict(&doc, doc.get(font).as_dict().unwrap());
        let (glyphs, spans) = replay(&show(&run, text, 12.0), &metrics, 12.0);
        (run, glyphs, spans)
    }

    #[test]
    fn shown_glyphs_land_where_the_shaper_put_them() {
        let Some(face) = sarabun("shown_glyphs_land_where_the_shaper_put_them") else { return };
        let (run, drawn, _) = shown(&face, "ที่ไม่ ป่า กำ");
        assert_eq!(drawn.len(), run.glyphs.len());
        let mut pen = 0.0;
        for (g, (id, x, y)) in run.glyphs.iter().zip(drawn) {
            assert_eq!(g.id, id);
            assert!(
                (x - (pen + g.dx)).abs() < 1e-3 && (y - g.dy).abs() < 1e-3,
                "glyph {id}: drawn at ({x}, {y}), shaped at ({}, {})",
                pen + g.dx,
                g.dy
            );
            pen += g.advance;
        }
    }

    #[test]
    fn only_clusters_to_unicode_cant_spell_get_actual_text() {
        let Some(face) = sarabun("only_clusters_to_unicode_cant_spell_get_actual_text") else { return };
        assert_eq!(shown(&face, "ทำงาน").2, ["ทำ"]);
        assert_eq!(shown(&face, "ที่ไม่").2, Vec::<String>::new());
        assert!(show(&ShapedRun::default(), "", 12.0).is_empty());
        // An absurd size never writes NaN or inf into the content stream (a raised, offset mark).
        let run = ShapedRun { glyphs: vec![ShapedGlyph { id: 5, advance: 0.0, width: 0.0, dx: -0.1, dy: 0.2, cluster: 0 }] };
        for size in [f64::NAN, f64::INFINITY] {
            let ops = String::from_utf8(show(&run, "่", size)).unwrap();
            assert!(!ops.contains("NaN") && !ops.contains("inf"), "{ops}");
        }
    }
}
