# Manual QA checklist

Run on Ubuntu 22.04, 24.04 and the newest LTS.

## Launch and files

- [ ] `docxlite` with no arguments shows the empty state.
- [ ] Open dialog (`Ctrl+O`) filters `.docx`, `.docm`, `.dotx`.
- [ ] Launching with a file argument opens it.
- [ ] Launching a second time with a file opens a new window in the same process.
- [ ] Drag-and-drop a `.docx` onto the window opens it.
- [ ] Dropping a non-docx file shows the friendly error, not a crash dialog.
- [ ] F5 reloads the current file.
- [ ] Closing the last window quits the app.

## Rendering

- [ ] Basic fixture: heading, bold, italic, red text, numbered list, nested lettered list, Symbol bullet.
- [ ] Resume fixture: header contact, right-tab date, two-column complex table stacked in reading order, simple table tabular, red image, footer.
- [ ] Table text is searchable and copyable.
- [ ] Header/footer text is searchable and copyable.
- [ ] Link text is blue/underlined; hover shows the real URL; click opens only http/https/mailto.
- [ ] Internal anchor link scrolls to the bookmark.
- [ ] EMF/WMF/SVG image shows a placeholder with alt text.
- [ ] Deleted revisions are hidden; inserted/moved-in text is shown.
- [ ] Text boxes render without duplicating `mc:Fallback` text.

## Search and zoom

- [ ] Search is case-insensitive.
- [ ] All matches highlight; current match is stronger; count says “1 of N”.
- [ ] Enter/Shift+Enter moves next/previous and wraps.
- [ ] Esc closes search.
- [ ] Zoom in/out/reset re-renders without losing text or links.
- [ ] Search after zoom still works.

## Themes and display

- [ ] Light theme is readable.
- [ ] Dark desktop theme: the app still uses a light/white-paper document view; explicit document colours are readable.
- [ ] HiDPI 2× renders correctly.
- [ ] Wayland and X11 both work.
- [ ] Window stays responsive while a 1,000-page document renders.

## Robustness

- [ ] Open 20 documents in a row; memory stays bounded.
- [ ] Truncated zip, wrong extension, empty file, directory, `/dev/zero`, FIFO, permission-denied file: friendly error, no crash.
- [ ] Password-protected/legacy `.doc`: friendly message.
- [ ] Circular styles/numbering, 10k-deep XML, huge image dimensions: no hang or crash.
- [ ] Kill/close during loading: app stays alive.

## Packaging

- [ ] `sudo apt install ./docxlite_*.deb` on clean 22.04 and 24.04.
- [ ] App appears in the launcher and Ubuntu Software metadata is valid.
- [ ] “Open With” on a `.docx` opens DocxLite.
- [ ] `sudo apt remove docxlite` removes the app; shared GTK libs remain.
- [ ] `desktop-file-validate` and `appstreamcli validate` pass.
