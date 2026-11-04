# Edited complex text may use fonts installed on this Mac, by their embedding permission

When an edit to existing text needs shaping and the PDF's embedded subset can't draw it, the edited paragraph is re-drawn with the full source face found on this computer (`PDFCRAFT_THAI_FONT_DIR`, then Font Book's `~/Library/Fonts` and `/Library/Fonts`; never other apps' data folders such as Office's cloud font cache, which made macOS ask for access to other apps' data), matched by family name without spaces or subset prefix and by style, when it covers the text and its OS/2 `fsType` allows installable or editable embedding; it is embedded whole. If any style of the paragraph has no such face and the text is Thai, the whole paragraph is set in Sarabun at the source face's visual size, with a notice. This personal fork deliberately departs from upstream, which embeds only open-licensed external fonts (`embedding::validate_document_input`): Thai documents from Word are set in Microsoft faces (Browallia New, Angsana New) that allow editable embedding, and following the font's own permission is the rule Word and Acrobat apply.

## Considered Options

- **Upstream's rule (open-licensed fonts only):** every edit in such documents would fall back to Sarabun, so edited paragraphs never match their neighbours.
- **Always Sarabun:** consistent, but the same mismatch.
- **Keep refusing:** no Thai editing at all in Word-made PDFs.

## Consequences

Edited PDFs carry whole copies of proprietary fonts whose licences permit it; restricted and preview-and-print-only fonts are never embedded (Sarabun is used instead).
