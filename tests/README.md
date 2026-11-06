# Test fixtures

- `valid/basic.docx` — hand-built fixture covering headings, bold/italic, colour, paragraphs, numbered/lettered Symbol bullet lists, a simple table, a hyperlink, a deleted revision, an insertion, and a header/footer.
- `valid/resume.docx` — hand-built fixture covering a right-aligned tab date, a complex two-column table with bullet lists in one cell and long text in the other (stacked rendering), a simple tabular table, an embedded PNG, and a header.

The core tests in `tests/core.rs` build additional small `.docx` files in the system temp directory at test time. This keeps the committed fixtures small.

Fixtures are generated from small XML templates and a generated PNG. Do not commit private documents. To create a new fixture:

1. Build the zip in a script using standard OOXML parts (`[Content_Types].xml`, `_rels/.rels`, `word/document.xml`, `word/_rels/document.xml.rels`).
2. Keep the content small and non-private.
3. Add the fixture to `tests/fixtures/valid/` and reference it in a test or the GUI smoke workflow.

Test commands:

```bash
cargo test
cargo test --no-default-features
DOCXLITE_SMOKE_EXIT=1 xvfb-run -a \
  env GDK_BACKEND=x11 GSETTINGS_BACKEND=memory GTK_A11Y=none NO_AT_BRIDGE=1 \
  target/debug/docxlite tests/fixtures/valid/basic.docx
```
