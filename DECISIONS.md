# Decisions

One line per non-obvious decision. The build specification is background; this file records what the implementation actually does.

- **Working name `docxlite`.** Kept for the local crate/binary; public name must change because `AnswerDotAI/docxlite` already exists on GitHub/PyPI. Rename gate is part of the release phase.
- **GTK4 only, no libadwaita, no WebKit.** `GtkTextView` provides text shaping, selection, copy, wrapping and search. `gtk4` is pinned to 0.11.5 with only the `v4_6` feature so the baseline stays GTK 4.6.
- **Tables are text-first.** A table is rendered as aligned text columns only when the whole table is simple (equal cells, no merges, one short single-line paragraph per cell, no lists/nested tables/images/line breaks). Otherwise the whole table is rendered in stacked reading order: each cell's blocks sequentially, blank-line separated, never interleaved columns. This preserves search/copy/zoom.
- **Table borders are not drawn.** Cell shading is retained through `TextTag` backgrounds. Borders would require a child-widget table, which would break search/copy/zoom.
- **Dedicated worker thread.** Loading/parsing runs on `std::thread`, not `gio::spawn_blocking` (Gio's I/O pool is shared and rate-limited). Results are delivered through `std::sync::mpsc` and polled by `glib::timeout_add_local`.
- **Panic containment.** The worker body is wrapped in `catch_unwind`; worker panics become `LoadError::Internal`. `panic = "unwind"` is intentional.
- **Chunked rendering uses `glib::idle_add_local`.** It is the repeating-source API; `idle_add_local_once` is one-shot where it exists and is not used for chunks.
- **Safe decompressor.** `zip` is built with `deflate-flate2` and a direct `flate2` dependency with the pure-Rust `rust_backend` (miniz_oxide). There is no `deflate-flate2-rust` feature in zip 9.4.1; the direct dependency selects the backend without adding a new crate.
- **Total resource budgets.** File 100 MB, XML part 32 MB, total decompressed 256 MB, nodes 2,000,000, depth 128, image 20 MB/32 MP, total image bytes 128 MB, rendered images 500, table cells 20,000. The 80 MB RAM target is a target, not a guarantee; measured RSS is documented in the README.
- **Headers/footers are rendered in v1.** Every unique referenced header/footer part is parsed and rendered once at the top/bottom, with a warning if multiple variants exist. Page-specific placement is not implemented.
- **Paragraph tab stops are parsed and applied.** Right/center/decimal tab stops work through Pango `TabArray`; right-aligned resume dates work.
- **Line spacing uses `GtkTextTag:line-height`.** Available since GTK 4.6 in the current binding. Exact/atLeast rules are approximated by the same factor.
- **Soft line wrapping is full-width in stacked tables.** This is intentional: wrapping inside a narrow column would require a widget table. In tabular mode cells are short by construction.
- **Light theme only.** The app forces the Adwaita light theme, a white paper document background and dark default text. This deliberately does not follow the desktop dark theme: Word documents commonly carry explicit black text, which was unreadable on a dark background.
- **Images decode after bounded reads and pixel checks.** Decoding is currently on the UI thread; the image-count and pixel caps bound it. Moving decode to the worker is a future hardening step.
- **CLI parser is hand-written.** No `clap`, `serde`, `anyhow` or async runtime. `--dump-text` never creates a GTK application, and the core tests run with `--no-default-features`.
- **Lints.** `unsafe_code`, `unwrap_used`, `expect_used`, `panic` and `indexing_slicing` are denied in library code. `arithmetic_side_effects` is deliberately not enabled; saturating/checked math is used instead. Test code has explicit allowances.
- **License is MIT.** The specification recommended MIT OR Apache-2.0 but left the choice to a human; MIT was chosen for simplicity. Switch before publishing if dual licensing is preferred.
- **Local build workaround.** On this machine the GTK4 runtime was present but `libgtk-4-dev` was missing and sudo was unavailable. The matching 24.04 dev packages were extracted under `.sysroot/` and `.cargo/config.toml` points `PKG_CONFIG_PATH` there. `.sysroot/` is gitignored; normal builders install `libgtk-4-dev`.

- **AppStream homepage is intentionally unset until the public repo/name exists.** `appstreamcli validate` reports one `url-homepage-missing` warning; add the final project URL before publishing.

- **Measured on this machine.** 833 KB stripped binary, 379 KB `.deb`, 880 KB installed; 1,000-page fixture parsed in 0.18 s with 260,000 nodes and ~70 MB headless RSS; GUI peak RSS under Xvfb was ~137 MB with `GSK_RENDERER=cairo` and ~213 MB with the default GL/llvmpipe path. The 80 MB GUI target is not met under Xvfb.

- **Local-only artifact baseline.** The checked-out local `.deb` was built on Ubuntu 24.04 (glibc 2.39/GTK 4.14) because no 22.04 container/sudo was available. The release workflow uses `ubuntu:22.04` for the older glibc/GTK baseline. The crate compiles with the `v4_6` feature, but the 22.04 runtime was not exercised locally.

- **Centred white page column.** The text view is wrapped in a responsive white page box capped at 816 px × zoom, centred over grey gutters. On narrow windows the page shrinks to the available width; a 200 ms resize poll adjusts the page width. Page padding is 64 px left/right and 48 px top/bottom, and paragraph margins are applied relative to that padding.
