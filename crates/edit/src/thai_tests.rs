//! Editing existing Thai text (docs/thai-forms-spec.md area D): installed full source faces, the
//! Sarabun fallback, Thai wrapping and condensing, and the สระอำ repair.

use super::*;

/// One page of Thai text ("ข้อความ" is bytes ABCDEFG) drawn with the TrueType fonts `bases` (page
/// resources F1, F2, …), each declaring `x_height` and embedding a program with no Thai glyphs
/// (like a producer's subset that lacks the edit's letters).
fn thai_page(bases: &[&str], x_height: u32, content: &str) -> Document {
    script_page(&['ข', '\u{0e49}', 'อ', 'ค', 'ว', 'า', 'ม'], bases, x_height, content)
}

/// [`thai_page`] with `letters` for the codes A, B, C, ….
fn script_page(letters: &[char], bases: &[&str], x_height: u32, content: &str) -> Document {
    let thai = letters;
    let mut cmap =
        String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n");
    cmap.push_str(&format!("{} beginbfchar\n<20> <0020>\n", thai.len() + 1));
    for (i, c) in thai.iter().enumerate() {
        cmap.push_str(&format!("<{:02X}> <{:04X}>\n", 0x41 + i, *c as u32));
    }
    cmap.push_str("endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let widths: Vec<&str> = (32..=71)
        .map(|c| {
            if c == 32 {
                "250"
            } else if c >= 65 {
                "500"
            } else {
                "0"
            }
        })
        .collect();
    let n = bases.len();
    // Objects: 1 catalog, 2 pages, 3 page, 4 content, 5 ToUnicode, 6 program, then per font a
    // font dictionary and its descriptor.
    let resources: Vec<String> = (0..n).map(|i| format!("/F{} {} 0 R", i + 1, 7 + 2 * i)).collect();
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents 4 0 R /Resources << /Font << {} >> >> >>", resources.join(" ")),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
        String::new(),
    ];
    for (i, base) in bases.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /{base} /FirstChar 32 /LastChar 71 /Widths [{}] /ToUnicode 5 0 R /FontDescriptor {} 0 R >>",
            widths.join(" "),
            8 + 2 * i
        ));
        objs.push(format!(
            "<< /Type /FontDescriptor /FontName /{base} /Flags 32 /FontBBox [-459 -335 791 921] /ItalicAngle 0 /Ascent 921 /Descent -335 /CapHeight 467 /XHeight {x_height} /StemV 56 /FontFile2 6 0 R >>"
        ));
    }
    let program: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        if i == 5 {
            out.extend_from_slice(format!("6 0 obj\n<< /Length {} /Length1 {} >>\nstream\n", program.len(), program.len()).as_bytes());
            out.extend_from_slice(program);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        } else {
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

/// The Thai font folder (`PDFCRAFT_THAI_FONT_DIR`: Sarabun, no Browallia), installed as this
/// process's search folders; `None` (test skipped) without it.
fn thai_font_dir(test: &str) -> Option<std::path::PathBuf> {
    use pdfcraft_fonts::{installed, thai};
    let dir = std::env::var_os(thai::DIR_VAR).map(std::path::PathBuf::from);
    let Some(dir) = dir else {
        eprintln!("skipping {test}: set {} to a folder with Sarabun to run it", thai::DIR_VAR);
        return None;
    };
    installed::install_font_dirs(vec![dir.clone()]);
    thai::install_document_faces(thai::find_document_faces(std::slice::from_ref(&dir)));
    Some(dir)
}

/// The decoded font programs (FontFile2) embedded in `doc`.
fn embedded_programs(doc: &Document) -> Vec<Vec<u8>> {
    doc.object_numbers()
        .into_iter()
        .filter_map(|num| {
            let o = doc.try_get(num).ok()?;
            let d = o.as_dict()?;
            (d.name(b"Type") == Some(b"FontDescriptor")).then_some(())?;
            let Object::Stream(s) = &*doc.resolve(d.get(b"FontFile2")?) else { return None };
            s.decoded().ok()
        })
        .collect()
}

#[test]
fn thai_edits_use_the_installed_full_source_face() {
    let Some(dir) = thai_font_dir("thai_edits_use_the_installed_full_source_face") else { return };
    let mut doc = thai_page(&["ABCDEF+Sarabun"], 500, "BT /F1 12 Tf 72 700 Td (ABCDEFG) Tj ET");
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[0].text, "ข้อความ");
    let edit = text::replace_block(&mut doc, 0, 0, "ข้อความใหม่ที่แก้ไข").unwrap();
    assert_eq!(edit.substituted, None);
    let doc = reopen(&doc);
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[0].text, "ข้อความใหม่ที่แก้ไข");
    // Drawn with the installed Sarabun Regular, embedded whole.
    let sarabun = std::fs::read(dir.join("Sarabun-Regular.ttf")).unwrap();
    assert!(embedded_programs(&doc).contains(&sarabun));
}

#[test]
fn without_the_full_source_face_thai_edits_set_the_paragraph_in_sarabun() {
    let Some(dir) = thai_font_dir("without_the_full_source_face_thai_edits_set_the_paragraph_in_sarabun") else { return };
    // Browallia New (Regular and a Bold run) isn't in the test's font folder.
    let mut doc = thai_page(&["ABCDEF+BrowalliaNew", "ABCDEF+BrowalliaNew-Bold"], 338, "BT /F1 12 Tf 72 700 Td (ABC) Tj /F2 12 Tf (DEFG) Tj ET");
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[0].text, "ข้อความ");
    let edit = text::replace_block(&mut doc, 0, 0, "ข้อความใหม่").unwrap();
    let notice = edit.notice.expect("a notice says Sarabun stood in");
    assert!(notice.contains("Sarabun") && notice.contains("BrowalliaNew"), "{notice}");
    let doc = reopen(&doc);
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[0].text, "ข้อความใหม่");
    // Every run of the paragraph is Sarabun, Regular and Bold as in the source, at the source's
    // visual size (Sarabun's x-height at the new size is the declared 0.338 em at 12 pt).
    let lines = text::text_lines(&doc, 0).unwrap();
    assert!(lines.iter().all(|l| l.base_font.contains("Sarabun")), "{:?}", lines.iter().map(|l| &l.base_font).collect::<Vec<_>>());
    let programs = embedded_programs(&doc);
    for style in ["Regular", "Bold"] {
        let file = std::fs::read(dir.join(format!("Sarabun-{style}.ttf"))).unwrap();
        assert!(programs.contains(&file), "Sarabun {style} is embedded");
    }
    let faces = pdfcraft_fonts::thai::installed_document_faces().unwrap();
    let x = pdfcraft_fonts::thai::x_height(&faces.regular).unwrap();
    for l in &lines {
        assert!((l.size * x - 12.0 * 0.338).abs() < 0.1, "size {}", l.size);
    }
}

#[test]
fn other_complex_scripts_without_a_full_face_are_still_refused() {
    if thai_font_dir("other_complex_scripts_without_a_full_face_are_still_refused").is_none() {
        return;
    }
    // Arabic (مرحبا is bytes ABCDE) in a subset that can't draw the edit, with no installed face:
    // Sarabun can't draw Arabic, so the edit is refused as before.
    let mut doc = script_page(&['م', 'ر', 'ح', 'ب', 'ا'], &["ABCDEF+NoSuchArabic"], 500, "BT /F1 12 Tf 72 700 Td (ABCDE) Tj ET");
    let before = page_content_bytes(&doc, 0);
    let err = text::replace_block(&mut doc, 0, 0, "مرحبا بكم").unwrap_err().to_string();
    assert!(!err.contains("Sarabun"), "{err}");
    assert_eq!(page_content_bytes(&doc, 0), before, "nothing changed");
}

/// The paragraph's rows on page 1 after an edit: (text, width) per visual line, top to bottom.
fn rows_of(doc: &Document) -> Vec<(String, f64)> {
    let mut lines = text::text_lines(doc, 0).unwrap();
    lines.sort_by(|a, b| b.rect[1].total_cmp(&a.rect[1]).then(a.rect[0].total_cmp(&b.rect[0])));
    let mut rows: Vec<(String, f64, f64, f64)> = Vec::new();
    for l in lines {
        match rows.last_mut() {
            Some(r) if (r.3 - l.rect[1]).abs() < 1.0 => {
                r.0.push_str(&l.text);
                r.1 = r.1.min(l.rect[0]);
                r.2 = r.2.max(l.rect[2]);
            }
            _ => rows.push((l.text.clone(), l.rect[0], l.rect[2], l.rect[1])),
        }
    }
    rows.into_iter().map(|r| (r.0, r.2 - r.1)).collect()
}

/// A one-line paragraph grows to the right, Thai like any other script (#1539), instead of being
/// condensed to its old width.
#[test]
fn a_slightly_longer_thai_edit_grows_its_line() {
    if thai_font_dir("a_slightly_longer_thai_edit_grows_its_line").is_none() {
        return;
    }
    let mut doc = thai_page(&["ABCDEF+Sarabun"], 500, "BT /F1 12 Tf 72 700 Td (ABCDEFG) Tj ET");
    let width = text::text_blocks(&doc, 0).unwrap()[0].rect;
    let width = width[2] - width[0];
    let edit = text::replace_block(&mut doc, 0, 0, "ข้อความดี").unwrap();
    assert_eq!(edit.notice, None);
    let rows = rows_of(&reopen(&doc));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "ข้อความดี");
    assert!(rows[0].1 > width + 0.5, "{} not grown past {width}", rows[0].1);
}

#[test]
fn a_much_longer_thai_edit_wraps_between_words() {
    if thai_font_dir("a_much_longer_thai_edit_wraps_between_words").is_none() {
        return;
    }
    // Longer than the line can grow (to 36 pt from the page's right edge).
    let words: Vec<&str> = ["ข้อความ", "ภาษา", "ไทย", "ที่", "ยาว", "มาก", "จริง", "ๆ"].iter().cycle().take(48).copied().collect();
    let mut doc = thai_page(&["ABCDEF+Sarabun"], 500, "BT /F1 12 Tf 72 700 Td (ABCDEFG) Tj ET");
    let edit = text::replace_block(&mut doc, 0, 0, &words.concat()).unwrap();
    assert!(edit.notice.is_some_and(|n| n.contains("overlap")), "a notice says to check the layout");
    let rows = rows_of(&reopen(&doc));
    assert!(rows.len() >= 2, "{rows:?}");
    assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<String>(), words.concat());
    let mut rest = words.as_slice();
    for (row, _) in &rows {
        let mut taken = String::new();
        while taken.len() < row.len() && !rest.is_empty() {
            taken.push_str(rest[0]);
            rest = &rest[1..];
        }
        assert_eq!(&taken, row, "a row breaks inside a word: {rows:?}");
    }
}

/// Word stores the nikhahit of สระอำ as a zero-width glyph mapped to a space, so the page reads
/// "ก าหนด". Sara aa never starts a Thai word: Edit text reads it as "กำหนด" and writes ำ back.
#[test]
fn sara_am_stored_as_space_and_sara_aa_reads_as_sara_am() {
    if thai_font_dir("sara_am_stored_as_space_and_sara_aa_reads_as_sara_am").is_none() {
        return;
    }
    let mut doc = script_page(&['ข', 'ก', ' ', 'า', 'ห', 'น', 'ด'], &["ABCDEF+Sarabun"], 500, "BT /F1 12 Tf 72 700 Td (ABCDEFG) Tj ET");
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[0].text, "ขกำหนด");
    text::replace_block(&mut doc, 0, 0, "ขกำหนดเอง").unwrap();
    assert_eq!(text::text_blocks(&reopen(&doc), 0).unwrap()[0].text, "ขกำหนดเอง");
}

#[test]
fn a_longer_thai_edit_keeps_a_paragraphs_line_count_by_condensing() {
    if thai_font_dir("a_longer_thai_edit_keeps_a_paragraphs_line_count_by_condensing").is_none() {
        return;
    }
    let mut doc = thai_page(&["ABCDEF+Sarabun"], 500, "BT /F1 12 Tf 72 700 Td (ABCDEFG) Tj ET BT /F1 12 Tf 72 685 Td (ABCDEFG) Tj ET");
    let blocks = text::text_blocks(&doc, 0).unwrap();
    assert_eq!((blocks.len(), blocks[0].rows.len()), (1, 2), "one paragraph of two lines");
    let width = blocks[0].rect[2] - blocks[0].rect[0];
    let edit = text::replace_block(&mut doc, 0, 0, "ข้อความข้อความดี").unwrap();
    assert_eq!(edit.notice, None, "no new line, so nothing to warn about");
    let rows = rows_of(&reopen(&doc));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<String>(), "ข้อความข้อความดี");
    assert!(rows.iter().all(|r| r.1 <= width + 0.5), "{rows:?} wider than {width}");
}

/// A paragraph whose first line starts at the right (the end of a line shared with other text,
/// as Word lays out runs): rewrapped, that line only has the room from where it starts.
#[test]
fn a_first_line_starting_mid_line_only_gets_the_room_left_on_it() {
    if thai_font_dir("a_first_line_starting_mid_line_only_gets_the_room_left_on_it").is_none() {
        return;
    }
    let mut doc =
        thai_page(&["ABCDEF+Sarabun"], 500, "BT /F1 12 Tf 230 700 Td (ABC) Tj ET BT /F1 12 Tf 72 685 Td (ABCDEFG ABCDEFG ABCDEFG ABCDEFG) Tj ET");
    let blocks = text::text_blocks(&doc, 0).unwrap();
    assert_eq!((blocks.len(), blocks[0].rows.len()), (1, 2), "one paragraph of two lines");
    let right = blocks[0].rect[2];
    // Joined text, as the automation tools pass it (no line breaks).
    text::replace_block(&mut doc, 0, 0, &format!("{}ดี", blocks[0].text)).unwrap();
    let lines = text::text_lines(&reopen(&doc), 0).unwrap();
    assert!(
        lines.iter().all(|l| l.rect[2] <= right + 0.5),
        "a line runs past the paragraph's right edge {right}: {:?}",
        lines.iter().map(|l| (l.text.clone(), l.rect)).collect::<Vec<_>>()
    );
}

/// Chrome marks each shaped Thai cluster with /ActualText ("น้ำ" drawn as น and a glyph with no
/// Unicode of its own): Edit text reads the marked text, leaves it alone (no sara am repair) and
/// writes an edit of the same length back as typed.
#[test]
fn thai_actual_text_from_chrome_reads_and_edits_unchanged() {
    if thai_font_dir("thai_actual_text_from_chrome_reads_and_edits_unchanged").is_none() {
        return;
    }
    // A น, B า (the drawn glyph, no tone mark), C เ, D ป, E น.
    let nam = "<FEFF0E190E490E33>";
    let pen = "<FEFF0E400E1B0E470E19>";
    let mut doc = script_page(
        &['น', 'า', 'เ', 'ป', 'น'],
        &["ABCDEF+Sarabun"],
        500,
        &format!("BT /F1 12 Tf 72 700 Td /Span << /ActualText {nam} >> BDC (AB) Tj EMC ( ) Tj /Span << /ActualText {pen} >> BDC (CDE) Tj EMC ET"),
    );
    assert_eq!(text::text_blocks(&doc, 0).unwrap()[0].text, "น้ำ เป็น");
    text::replace_block(&mut doc, 0, 0, "เป็น น้ำ").unwrap();
    assert_eq!(text::text_blocks(&reopen(&doc), 0).unwrap()[0].text, "เป็น น้ำ");
}
