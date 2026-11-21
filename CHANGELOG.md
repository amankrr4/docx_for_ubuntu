# Changelog

## 0.1.0

- Initial read-only viewer for `.docx`, `.docm` and `.dotx`.
- Text, styles, headings, lists, tables, images, hyperlinks, text boxes, headers/footers.
- Text-first tables with a stacked fallback for complex cells.
- Search, copy, drag-and-drop, open dialog, multi-window, F5 reload.
- Static zoom: the page scales as a whole without re-flowing text or moving line breaks.
- Papers-style zoom menu: Fit Page, Fit Width, Automatic and 50–400% presets.
- Link hover and clicks are converted to buffer coordinates so they line up at any zoom and page padding.
- Safe read-only loader: bounded zip reads, DTD rejection, resource budgets, panic containment.
- Headless `--dump-text`, `--verbose` and `--stats`.
- Core tests, random-byte property test, GUI smoke path.
- `.deb`, desktop file, AppStream metadata and scalable icon.
