# PdfCraft forms: Thai text

Filling AcroForm fields with Thai text so it reads correctly both inside PdfCraft and in the saved PDF.

## Language

**Field value**:
The text a field holds (`/V`), independent of how it looks.
_Avoid_: content, data

**Appearance**:
The drawing of a field that viewers show and print (its normal appearance stream). It is regenerated from the field value whenever the value is set.
_Avoid_: rendering, AP (in prose)

**Fill editor**:
The in-app box the user types a field value into while filling; it is drawn with interface fonts, not the field's font.
_Avoid_: overlay, text box

**Thai document face**:
The Thai typeface used to draw Thai field values into the appearance (Sarabun, Regular and Bold).
_Avoid_: Thai font (ambiguous)

**Thai interface face**:
The Thai typeface the interface, including the fill editor, uses for Thai characters: whichever one PdfCraft itself chooses. It may differ from the Thai document face; only the line pitch has to match the page.
_Avoid_: UI font

**Source face**:
The font a PDF's existing text is drawn with. Producers such as Word usually embed only a subset of it (the letters used), so it often can't draw edited text.
_Avoid_: original font, PDF font

**Full source face**:
A complete copy of the source face's family and style installed on this computer (Font Book), able to draw any edit while looking the same.

**Fallback face**:
The Thai document face (Sarabun) used for edited text when no full source face is available; it replaces the font of the whole edited paragraph block, at the source face's visual size.

**Line pitch**:
The distance from one baseline to the next in a multi-line field. It is the same for every font at a given size, so fields set side by side as a table line up row for row. A Thai first line may sit a little lower than a Latin one, just enough to keep its tallest stacked marks (ื่) inside the field; the rows below keep the same pitch.
_Avoid_: leading, line height

**Comb cell**:
One box of a comb field; it holds one grapheme cluster (ที่ is one cell), and `MaxLen` counts cells.
_Avoid_: character slot

**Shaping**:
Turning a run of Thai characters into positioned glyphs, so stacked marks, descender-less consonants and สระอำ come out right.
