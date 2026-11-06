# Security Policy

DocxLite reads untrusted Word documents. It is designed to be read-only, offline and non-executing.

## Reporting a vulnerability

Please use GitHub's **private vulnerability reporting** for this repository. Do not open a public issue for a suspected vulnerability.

Include:

- affected version,
- Ubuntu release and display server,
- a minimal, non-private reproducer if possible,
- the category of issue (crash, hang, memory exhaustion, unsafe file access, link handling, etc.).

**Do not attach a private document.** Reduce the issue to a small generated file first. If you cannot, describe the structure and contact the maintainer privately.

## Scope

In scope: parser crashes/panics, zip/XML resource exhaustion, path traversal attempts, unsafe link opening, external entity/DTD handling, unexpected network access, reading outside the opened document.

Out of scope: visual fidelity differences, missing office features, fonts not installed, disabled/unsupported document formats, and issues in GTK/GDK/Pango/gdk-pixbuf that are not reachable through DocxLite's usage.

## Guarantees

- The user's file is never written to.
- Zip entries are never extracted to disk.
- XML DTDs are rejected; external entities are not resolved.
- No network sockets, telemetry, update checks or external image/template loading.
- Links open only after an explicit click and only for `http`, `https` or `mailto`.
