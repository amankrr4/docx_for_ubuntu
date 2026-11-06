#!/bin/sh
set -eu

# Install DocxLite into ~/.local (or $PREFIX).
PREFIX="${PREFIX:-$HOME/.local}"
HERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

install -Dm755 "$HERE/docxlite" "$PREFIX/bin/docxlite"
install -Dm644 "$HERE/docxlite.desktop" "$PREFIX/share/applications/docxlite.desktop"
install -Dm644 "$HERE/io.github.docxlite.DocxLite.svg" "$PREFIX/share/icons/hicolor/scalable/apps/io.github.docxlite.DocxLite.svg"
install -Dm644 "$HERE/io.github.docxlite.DocxLite.metainfo.xml" "$PREFIX/share/metainfo/io.github.docxlite.DocxLite.metainfo.xml"

echo "Installed DocxLite to $PREFIX"
echo "Make sure $PREFIX/bin is in your PATH."
