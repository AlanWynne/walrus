---
name: "Walrus rust-walrus-notes"
description: "Use when changing Walrus Rust code, crate APIs, splice replay, or project-specific Rust types."
applyTo: "**/*.rs"
---

# Walrus Rust notes (project-specific, Tier 3)

The generic Rust coding standards are shared from the central
`rust-standards.md` (identical across all Rust projects). This file holds ONLY
the Walrus-specific Rust guidance that was previously woven into Walrus's own
copy of the standards, separated out so the shared standard stays identical
everywhere.

## Crate-specific error handling
- Library code is `walrus-fmt` (`crates/walrus`): `thiserror`, one `Error` enum
  per crate. See `crates/walrus/src/error.rs`.
- Application/binary code is `walx` and `walker`: `anyhow` with `.context(...)`.

## Ownership (Walrus hot path)
- Splice replay is a hot loop -- comment any `clone()` inside it.

## Data modelling (Walrus types)
- Data-carrying enums model real Walrus states: the `Version` addressing enum and
  the change-record `op` variants -- keep illegal states unrepresentable there.
- Newtypes clarify a version index / byte offset.
- `#[non_exhaustive]` applies to the record and manifest enums because the format
  is a 0.1 draft that may gain variants (see FORMAT.md).

## Module boundary
- Keep the `walrus-fmt` public surface small and documented -- it is the
  embeddable library. The dependency boundary is detailed in
  `project-conventions.md` (invariant 2).

## Note on homogenisation
Walrus's previous standalone `rust-standards.md` phrased a couple of generic
rules slightly more softly (e.g. "should exceed 400", "aim for ~40 lines"). The
shared central standard is the authority now and uses the firmer wording
("No source file may exceed 400 lines"). This is a deliberate homogenisation, not
an accident.
