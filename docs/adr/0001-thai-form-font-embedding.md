# Thai field values are drawn in a fully embedded Sarabun TrueType font, found at runtime first

When a form field value contains Thai, its appearance is shaped with harfrust and drawn in Sarabun, embedded whole as a CIDFontType2 (Identity-H, ToUnicode) once per document in `/DR`, and the field's `/DA` font name is rewritten to it. The font file is looked up at runtime first (`PDFCRAFT_THAI_FONT_DIR`, then `~/Library/Fonts`, `/Library/Fonts`) and falls back to a craft-fonts face compiled into the build. This is a personal fork, so it deliberately departs from upstream's pattern (Type 3 outlines from craft-fonts only, as for Japanese edited text): a real embedded font lets Acrobat and Preview redraw the field without losing Thai, has no 240-glyph cap, and works with shaped glyph IDs directly; embedding the whole ~90 KB face avoids writing a subsetter.

## Considered Options

- **Type 3 of shaped glyphs (upstream's Japanese path):** simpler, but other viewers redraw the field from `/DA` and lose the Thai.
- **Leave `/DA` alone, only change the appearance:** less intrusive, same loss on redraw.
- **craft-fonts only:** upstream-compliant, but needs a rebuild to change faces; kept as the fallback.

## Consequences

Saved PDFs carry Sarabun (OFL-1.1) permanently. Without any Thai face the value is still saved, the appearance falls back to Helvetica `?`, and the user is warned.
