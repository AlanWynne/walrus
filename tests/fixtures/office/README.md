# Office CLI fixtures

These deterministic, minimal ODF text and OOXML Word fixtures exercise content
detection and exact ZIP-byte round trips through the public library and CLI.
They are generated locally with fixed ZIP timestamps and original XML content;
they contain no third-party document content.

- `tiny.odt`: ODF text document with a stored first `mimetype` member.
- `tiny-updated.odt`: same package layout with changed `content.xml`.
- `tiny.docx`: minimal OOXML package with content types, package relationships,
  and a WordprocessingML document part.
