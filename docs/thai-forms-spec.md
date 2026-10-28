# Thai forms: rebuild spec

What this personal fork adds to PdfCraft so Thai can be filled into forms, typed into comments and edited in existing PDFs on macOS, written so it can be rebuilt on any upstream release (ADR 0003). Terms in **bold** are defined in `GLOSSARY.md`.

Status: written 2026-10-28 for the rebuild on v0.14.0, from the v0.9.0 fork (tag `thai-forms-v0.9`). Areas A and B rebuilt on v0.14.0 (branch `thai`); C and D not yet.

## Principles

1. **Behaviour, not code.** Each rule below is met however the current upstream is laid out. "Last seen in" hints point at the most recent build of each area only to help find the seam.
2. **Upstream first.** If upstream already meets a criterion (Thai shaping, Thai line breaking, its craft-fonts Thai face, its interface Thai font, reading `/ActualText`), use it and write no fork code for it. Check before building each rule: `git grep -i thai <tag> -- crates`.
3. **Deliberate departures stay.** These are hard requirements even if upstream does otherwise:
   - ADR 0001: Thai field values are drawn in Sarabun, found at runtime first and embedded whole as a real TrueType font, so other viewers can redraw the field.
   - ADR 0002: Edit text may embed fonts installed on this Mac when their OS/2 `fsType` allows it.
4. **Upstream's repo rules still apply**: never panic, quality gates, scoped builds. Its font rule (only open-licensed fonts from craft-fonts may enter a PDF) does not (ADRs 0001, 0002).
5. **Each area ships on its own**, in the order A → B → C → D.

## Rebuild playbook

1. `git fetch origin --tags`, then cut `thai` (or `thai-<version>`) from the release **tag**, not `origin/main`.
2. First commit: copy `GLOSSARY.md`, `docs/adr/0001…0003` and this spec from the previous branch.
3. craft-fonts: merge `origin/main` into the local `thai` branch. It adds Sarabun (Regular, Bold, Italic, Bold Italic, OFL) and lists Sarabun **before** Noto Sans Thai in the manifest, so Sarabun is the built-in fallback and the face upstream takes first. Build with `CRAFT_FONTS_DIR` set to its **absolute** path (build.rs runs in the crate's folder, so a relative path silently builds without craft-fonts).
4. For each area: read upstream's code for that feature, list what it already does against the acceptance examples, and build only the rest. Commit as `Thai forms N: <behaviour>`.
5. Before the first build, record upstream's own test failures on the plain tag (`cargo test -p <crate>`); only failures that don't happen there count against the rebuild.
6. Verify (see Verification), then release (see Release).

## Fonts the fork finds

- The **Thai document face** is Sarabun, looked up by name-table family in `PDFCRAFT_THAI_FONT_DIR`, then `~/Library/Fonts`, then `/Library/Fonts`, then the faces craft-fonts tags as Thai. Skip files over 32 MB, files that don't parse and faces without ก.
- Never read other apps' data folders (Office's cloud font cache): macOS asks the user for access to other apps' data. A font only Office has must be installed with Font Book first.
- The desktop app and the CLI both install these lookups at start-up.
- For development, the Sarabun files are in `../thai-fonts/` (`PDFCRAFT_THAI_FONT_DIR=../thai-fonts`).

## A. Filling form fields

### Rules

- **When Sarabun takes over.** A field value is drawn in the Thai document face only when the field's own font can't draw the text **and** Sarabun covers all of it. Otherwise upstream's behaviour stands, including its undrawn-text reporting. A field already in Sarabun stays in it for a Latin value.
- **A real embedded font (ADR 0001).** The text is shaped, and the face is embedded whole as Type 0 / CIDFontType2 (Identity-H, full widths, ToUnicode from the cmap plus the alternate glyphs shaping produces). It is embedded once per document in `/DR` and reused after save and reopen (same BaseFont, identical font file).
- **`/DA` is rewritten** to the Sarabun resource, keeping the size and colour, so Acrobat and Preview redraw the field in Thai.
- **Style follows the field font**: bold → Sarabun Bold, italic → Italic, bold italic → Bold Italic, and Regular when the style is missing.
- **Visual size.** When a field switches to Sarabun, its size is scaled so Sarabun's x-height matches the x-height the field font declares (FontDescriptor `/XHeight`). The scale is clamped to 0.5–1.5 and rounded to 0.1 pt, and `/DA` records the new size. A font with no declared x-height (standard Helvetica) keeps its size.
- **Line pitch.** In every multiline field, Thai or not, the first baseline is 0.85 em below the top of the text area and the **line pitch** is 1.15 em, so a table made of fields side by side lines up row for row.
- **Stacked marks.** A Thai first line sits at whichever is lower: 0.85 em, or just low enough for the face's tallest shaped stack (ื่, about 1.12 em in Sarabun) to stay inside the box. The pitch stays 1.15 em.
- **Wrapping.** Multiline Thai wraps at dictionary word boundaries.
- **Auto size.** A field with size 0 Tf is sized with the Thai pen's widths, word breaks and line pitch. New fields default to Auto size (upstream).
- **Comb and MaxLen.** A **comb cell** holds one grapheme cluster, and `/MaxLen` counts clusters, both in the appearance and in the fill editor.
- **Choices.** Combo and list options in Thai are drawn the same way.
- **No Thai face at all.** The value is still saved, the appearance draws `?`, and the user is warned (a toast in the app, a warning in the CLI log).
- **Reading back.** Text extraction and copy keep combining marks after the glyph they sit on, so shaped Thai reads back in logical order. Clusters ToUnicode can't spell (สระอำ) carry `/ActualText`.

### Acceptance examples

- Fill "ที่อยู่" into a Helvetica field, save, then reopen in PdfCraft and in Preview: it reads the same, and the extracted text is "ที่อยู่".
- Fill two Thai fields in one document: the PDF holds one Sarabun font program.
- A BrowalliaUPC 12 field filled with Thai comes out as Sarabun 8.1 and doesn't overflow.
- A Helvetica Bold field filled with Thai uses Sarabun Bold.
- "หมื่น" in the first line of a multiline field at the largest Auto size: no mark is clipped.
- A row of multiline fields (code, description, quantity, price, amount at 8 pt), with Thai in the description: the baselines of rows 2 and later line up across the fields.
- A comb field with MaxLen 2: "ที่นี่" fills both cells (ที่ | นี่), and typing a third cluster is refused.
- With no Thai face available, saving keeps `/V` and shows the warning.
- "น้ำ" extracts as "น้ำ", not "น ้ำ" or with the marks reordered.

Last seen in (v0.14.0): `fonts/src/thai.rs` (lookup, shape, embed, `embedded_face`, x_height, mark_top, wrap). `forms/src/appearance.rs`: `FontText::Thai` (upstream's field-font enum gained a variant, so `/DR` lookup, widths, wrapping and overflow all see Sarabun) and `field_appearance_written`, the one step that embeds and rewrites `/DA` before upstream's read-only drawing; `redraw` (lib.rs) and `redraw_widgets` (author.rs) call it. `forms/src/lib.rs` (field_len, clip_to_max_len). `engine` (install_thai_faces), `render/src/text.rs` (mark order). Tests: `forms/src/thai_tests.rs`, and the ones that render in `engine/tests/it/thai_forms.rs` (forms may not depend on render: `cargo xtask layers`).
Upstream at v0.14.0 has its own Thai shaping (`fonts/src/shaping.rs`), but only with the built-in craft-fonts face, so it can't serve a runtime-found Sarabun.

## B. Interface and fill editor

### Rules

- The **Thai interface face** is whatever upstream gives the interface. The fork adds no interface font code.
- The **fill editor** lays out multiline text one line every 1.15 em, as on the page, and the on-screen Auto size uses the same pitch, so the column being typed doesn't drift from the fields beside it.
- The fill editor counts MaxLen in grapheme clusters (see A).

### Acceptance examples

- Thai shows in menus, dialogs and the fill editor (no tofu).
- While typing three lines of Thai into a multiline field next to Latin fields, each line stays level with its neighbours, before and after the field is committed.

Last seen in (v0.14.0): `ui-egui/src/forms_ui.rs` (LINE_PITCH, pitched_job, MaxLen clip after each edit).

## C. Comments: text boxes, callouts, Fill & Sign text

### Rules

- A FreeText comment whose text the standard fonts can't draw, and that the Thai document face covers in full, is drawn in that face: shaped, wrapped at word boundaries, with its glyphs in ToUnicode. The face is embedded once per document and reused.
- Such text isn't reported as undrawable, so it isn't marked undrawn, restyling and flattening aren't refused, and no `?` notice appears.
- Text Sarabun can't draw (Chinese, say) keeps upstream's behaviour.
- Add Text (new page text) is upstream's and isn't touched; upstream draws Thai there with its craft-fonts Thai face, which the craft-fonts manifest makes Sarabun.

### Acceptance examples

- A text box, a callout and a Fill & Sign text holding "สวัสดีครับ" render correctly, flatten without a refusal, and read back as typed.
- A text box holding Chinese behaves exactly as on plain upstream.

Last seen in (v0.9.0): `annot/src/appearance.rs`, `annot/src/lib.rs`.

## D. Editing existing Thai text

### Rules

- **Installed full source faces (ADR 0002).** When an edit needs shaping and the PDF's embedded subset can't draw it, look for the **full source face** in the font folders above. Match by family name with the subset prefix and spaces removed, and by style suffix. The font must cover the edit and its `fsType` must allow installable or editable embedding. It is embedded whole and draws every character of that run, so no Thai cluster is split between it and the subset.
- **Fallback face.** If any run of the paragraph has no such face and the text is Thai, the whole paragraph is set in Sarabun, per run style, at the source face's x-height, with a notice in the interface. Restricted and preview-and-print-only fonts are never embedded.
- **"The run's own font can draw it"** means the source encoding covers the text, or the embedded program is not a subset and is licensed for editing (upstream's rule).
- **Fitting.**
  - A one-line paragraph grows to the right (upstream), Thai included.
  - A paragraph of several lines keeps its line count: an edit that would need more lines is condensed, down to 80%. Only beyond that does it wrap onto a new line, with a notice.
  - A first row that starts mid-line (Word's runs) only gets the room from where it starts to the paragraph's right edge.
- **Wrapping** at Thai dictionary word boundaries, combined with upstream's other line-break rules: a row may end wherever either allows. Thai rows join without a space when read back. A wrapped edit therefore drops the space at the wrap; that's by design.
- **Untouched rows** keep their source operators only when the paragraph is not in Sarabun and not condensed.
- **Sara am repair.** Word stores the nikhahit of สระอำ as a zero-width glyph mapped to a space, so the page reads "ก าหนด". Edit text reads a Thai character, a space and sara aa as สระอำ, because sara aa never starts a Thai word. Text that already reads correctly through `/ActualText` (Chrome) is left alone.
- **Mixed fonts in a row.** A row may change font or style between grapheme clusters of left-to-right text. Right-to-left rows and rows that would split a cluster are still refused.

### Acceptance examples

- In a Word-made Thai PDF set in Browallia New, with Browallia New installed in Font Book: edit a word, and the paragraph keeps Browallia New.
- The same PDF with the font not installed: the whole paragraph turns Sarabun at the same visual size, and a notice appears.
- A three-line Thai paragraph edited to slightly longer text stays three lines (condensed); much longer text wraps onto a fourth line, with a notice.
- A slightly longer one-line Thai edit grows to the right.
- Word's "ก าหนด" reads as "กำหนด" in the editor, and saving writes "กำหนด".
- Chrome-made "น้ำ" and "เป็น" (ActualText) edit and read back as typed.
- No macOS "access data from other apps" prompt at start-up.

Last seen in (v0.9.0): `fonts/src/installed.rs`, `fonts/src/thai.rs` (repair_sara_am), `edit/src/text_regions.rs` (MIN_FIT, wrap), `edit/src/text.rs`, `engine` (edit_font_notices).

## Out of scope

- The Thai interface language (upstream ships it and follows the macOS language).
- Add Text (upstream's).
- Subsetting Sarabun or installed fonts: they're embedded whole.
- Font lookup outside macOS folders, and other scripts.
- Adding Thai to upstream's non-embedded-font substitution chain (reporting only).

## Verification

- **Automated.** Each acceptance example becomes a test that builds its own synthetic PDF in code. Tests needing Sarabun skip when no face is found, like upstream's craft-fonts tests. Prove each test fails with the behaviour reverted.
- **Quality gates** per repo rules: fmt, clippy `-D warnings`, scoped tests, `cargo xtask layers`, `cargo xtask assets`.
- **Manual smoke checklist** (private PDFs kept outside the repo in `../thai-out/`, never committed):
  - a Word-made Thai form (untouched copy): fill fields, edit a paragraph, save, then open in Preview;
  - a Thai invoice from Apple pstopdf with a table of 8 pt multiline fields: rows line up, and the amount in words (12 pt, หมื่น) isn't clipped;
  - render with `pdfcraft-cli render out.pdf --page 1 --dpi 144 --out x.png` and compare with the previous version's renders.

## Release

1. Push the branch to `mine`, set `mine/main` to the upstream tag, and push the tag.
2. Push craft-fonts `thai` to `mine`.
3. Build the DMG with `CRAFT_FONTS_DIR=../craft-fonts packaging/macos/package.sh --arch aarch64`, and install it into /Applications after asking the user to close PdfCraft.
4. Report the size of `target/` and offer `cargo clean`.
