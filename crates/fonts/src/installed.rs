//! Full faces installed on this computer, for redrawing edited text whose PDF font is a subset.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use skrifa::FontRef;
use skrifa::string::StringId;

use crate::thai::Style;

/// Faces tried in a collection (`.ttc`).
const MAX_FACES: u32 = 16;

/// A complete font file matching a PDF font's family and style.
#[derive(Clone, Debug)]
pub struct FullFace {
    pub family: String,
    pub style: Style,
    pub bytes: Arc<[u8]>,
    pub index: u32,
    pub path: PathBuf,
}

/// Why no full face can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// No installed file has this family and style.
    NotFound,
    /// The file's embedding permission (OS/2 fsType) forbids embedding it in an edited document.
    NotPermitted,
    /// The file lacks a character of the text.
    NotCovered,
}

/// The installed font files, indexed by family and style.
#[derive(Clone, Debug, Default)]
pub struct FontIndex {
    faces: Vec<(String, Style, PathBuf, u32)>,
}

impl FontIndex {
    /// Index every font in `dirs`.
    pub fn scan(dirs: &[PathBuf]) -> Self {
        let mut faces = Vec::new();
        for path in crate::thai::font_files(dirs) {
            let Some(bytes) = crate::thai::read_font(&path) else { continue };
            for index in 0..MAX_FACES {
                let Ok(font) = FontRef::from_index(&bytes, index) else { break };
                let family = crate::thai::name(&font, StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME);
                let style = crate::thai::name(&font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME)
                    .and_then(|s| crate::thai::style_named(&s));
                if let (Some(family), Some(style)) = (family, style) {
                    faces.push((family, style, path.clone(), index));
                }
            }
        }
        Self { faces }
    }

    /// The families with a Regular face that may be embedded (a single font file, its fsType
    /// allowing installable or editable embedding), sorted: what a font list may offer.
    pub fn embeddable_families(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .faces
            .iter()
            .filter(|(_, style, _, index)| *style == Style::Regular && *index == 0)
            .filter(|(_, _, path, _)| crate::thai::read_font(path).is_some_and(|b| !b.starts_with(b"ttcf") && may_embed(&b)))
            .map(|(name, _, _, _)| name.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The full face for the PDF font `pdf_name` (e.g. `ABCDEF+BrowalliaNew-Bold`) that may be
    /// embedded and covers `text`.
    pub fn find(&self, pdf_name: &str, text: &str) -> Result<FullFace, Missing> {
        let (family, style) = family_and_style(pdf_name);
        let key = crate::embedding::face_key(family);
        let mut why = Missing::NotFound;
        for (name, _, path, index) in self.faces.iter().filter(|(name, s, _, _)| *s == style && crate::embedding::face_key(name) == key) {
            // FontFile2 holds a single font program, not a collection.
            let Some(bytes) = crate::thai::read_font(path).filter(|b| *index == 0 && !b.starts_with(b"ttcf")) else { continue };
            if !may_embed(&bytes) {
                why = Missing::NotPermitted;
                continue;
            }
            if !crate::embedding::covers(&bytes, text) {
                if why == Missing::NotFound {
                    why = Missing::NotCovered;
                }
                continue;
            }
            return Ok(FullFace { family: name.clone(), style, bytes, index: *index, path: path.clone() });
        }
        Err(why)
    }
}

/// A PDF font name's family and style: `ABCDEF+BrowalliaNew-BoldItalic` → (`BrowalliaNew`,
/// Bold Italic), `Calibri,Bold` → (`Calibri`, Bold); no style suffix is Regular.
fn family_and_style(pdf_name: &str) -> (&str, Style) {
    let name = pdf_name.rsplit_once('+').map_or(pdf_name, |(_, n)| n);
    let style_of = |suffix: &str| match suffix.to_ascii_lowercase().as_str() {
        "regular" | "roman" | "normal" | "book" => Some(Style::Regular),
        "bold" => Some(Style::Bold),
        "italic" | "oblique" => Some(Style::Italic),
        "bolditalic" | "boldoblique" => Some(Style::BoldItalic),
        _ => None,
    };
    for sep in [',', '-'] {
        if let Some((family, suffix)) = name.rsplit_once(sep)
            && let Some(style) = style_of(suffix)
        {
            return (family, style);
        }
    }
    (name, Style::Regular)
}

/// Whether the font's embedding permission (OS/2 fsType) allows embedding it in a document that
/// is then edited: installable or editable, with outlines (not bitmap-only).
fn may_embed(bytes: &[u8]) -> bool {
    use skrifa::raw::TableProvider as _;
    let Ok(font) = FontRef::new(bytes) else { return false };
    // No OS/2 table: no restriction is declared.
    let Ok(os2) = font.os2() else { return true };
    let fs = os2.fs_type();
    const RESTRICTED: u16 = 0x0002;
    const PREVIEW_PRINT: u16 = 0x0004;
    const EDITABLE: u16 = 0x0008;
    const BITMAP_ONLY: u16 = 0x0200;
    // When several bits are set the least restrictive applies (OpenType OS/2 fsType).
    fs & BITMAP_ONLY == 0 && (fs & EDITABLE != 0 || fs & (RESTRICTED | PREVIEW_PRINT) == 0)
}

/// The folders the app searches, installed at start-up; indexed on first use.
static DIRS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
static INDEX: std::sync::OnceLock<FontIndex> = std::sync::OnceLock::new();

/// Make `dirs` the folders searched for full faces in this process. Only the first call counts;
/// returns whether this one did.
pub fn install_font_dirs(dirs: Vec<PathBuf>) -> bool {
    DIRS.set(dirs).is_ok()
}

/// The index of the installed folders (`None` when none were installed).
pub fn index() -> Option<&'static FontIndex> {
    let dirs = DIRS.get()?;
    Some(INDEX.get_or_init(|| FontIndex::scan(dirs)))
}

/// The folders searched for full faces: [`crate::thai::search_dirs`] (`PDFCRAFT_THAI_FONT_DIR`,
/// then Font Book's `~/Library/Fonts` and `/Library/Fonts`). Other apps' data folders (Office's
/// cloud font cache) are never read: macOS would ask the user to let PdfCraft access other apps'
/// data. A font only Office has must be installed with Font Book first.
pub fn search_dirs(env_dir: Option<&OsStr>, home: Option<&Path>) -> Vec<PathBuf> {
    crate::thai::search_dirs(env_dir, home)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thai_fonts(test: &str) -> Option<PathBuf> {
        let dir = std::env::var_os(crate::thai::DIR_VAR).map(PathBuf::from);
        if dir.is_none() {
            eprintln!("skipping {test}: set {} to a folder with Sarabun to run it", crate::thai::DIR_VAR);
        }
        dir
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-installed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_pdf_font_name_finds_its_family_and_style() {
        let Some(dir) = thai_fonts("a_pdf_font_name_finds_its_family_and_style") else { return };
        let index = FontIndex::scan(&[dir]);
        for (name, style) in [
            ("ABCDEF+Sarabun-Bold", Style::Bold),
            ("ABCDEF+Sarabun", Style::Regular),
            ("Sarabun-BoldItalic", Style::BoldItalic),
            ("Sarabun,Italic", Style::Italic),
        ] {
            let face = index.find(name, "ข้อความ").unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!((face.family.as_str(), face.style), ("Sarabun", style), "{name}");
        }
        assert_eq!(index.find("ABCDEF+BrowalliaNew", "ข้อความ").map(|f| f.style), Err(Missing::NotFound));
        assert_eq!(index.find("ABCDEF+Sarabun", "日本").map(|f| f.style), Err(Missing::NotCovered));
    }

    #[test]
    fn a_font_that_forbids_embedding_is_refused() {
        let Some(src) = thai_fonts("a_font_that_forbids_embedding_is_refused") else { return };
        let dir = temp_dir("restricted");
        let mut bytes = std::fs::read(src.join("Sarabun-Regular.ttf")).unwrap();
        set_fs_type(&mut bytes, 0x0002);
        std::fs::write(dir.join("Sarabun-Regular.ttf"), &bytes).unwrap();
        let index = FontIndex::scan(std::slice::from_ref(&dir));
        assert_eq!(index.find("ABCDEF+Sarabun", "ข้อความ").map(|f| f.style), Err(Missing::NotPermitted));
        // Editable embedding (Browallia New's) is fine.
        set_fs_type(&mut bytes, 0x0008);
        std::fs::write(dir.join("Sarabun-Regular.ttf"), &bytes).unwrap();
        assert!(FontIndex::scan(std::slice::from_ref(&dir)).find("ABCDEF+Sarabun", "ข้อความ").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Overwrite the OS/2 table's fsType (the checksum isn't checked by readers that matter here).
    fn set_fs_type(bytes: &mut [u8], fs_type: u16) {
        let tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        for i in 0..tables {
            let rec = 12 + 16 * i;
            if &bytes[rec..rec + 4] == b"OS/2" {
                let off = u32::from_be_bytes(bytes[rec + 8..rec + 12].try_into().unwrap()) as usize;
                bytes[off + 8..off + 10].copy_from_slice(&fs_type.to_be_bytes());
                return;
            }
        }
        panic!("no OS/2 table");
    }

    #[test]
    fn other_apps_data_folders_are_never_searched() {
        // Reading another app's container (Office's cloud font cache) makes macOS ask the user
        // whether PdfCraft may access other apps' data: only Font Book's folders are searched.
        let home = temp_dir("home");
        let cache = home.join("Library/Group Containers/UBF8T346G9.Office/FontCache/4/CloudFonts");
        std::fs::create_dir_all(cache.join("Browallia New")).unwrap();
        let dirs = search_dirs(None, Some(&home));
        assert_eq!(dirs, [home.join("Library/Fonts"), PathBuf::from("/Library/Fonts")]);
        assert!(dirs.iter().all(|d| !d.starts_with(home.join("Library/Group Containers"))));
        let _ = std::fs::remove_dir_all(&home);
    }
}
