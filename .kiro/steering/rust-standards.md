---
inclusion: always
---

# Rust Coding Standards and File Structure

Apply to every Rust file in this workspace without being asked.

## Naming

| Item | Convention | Example |
|------|-----------|---------|
| Types, traits, enums | `UpperCamelCase` | `WalrusFile` |
| Functions, methods, variables | `snake_case` | `reconstruct_version` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_LOG_RECORDS` |
| Modules | `snake_case` | `reconstruct` |

- Prefer full names: `version_index` not `vi`.
- Booleans read as predicates: `is_reverse_log`, `has_checkpoints`.

## Error Handling

- Library code (`walrus-fmt`): `thiserror`, one `Error` enum per crate with
  enough context. See `crates/walrus/src/error.rs`.
- Application/binary code (`walx`, `walker`): `anyhow` with `.context("...")` on
  every `?` crossing a module boundary.
- Never `unwrap()` / `expect()` in library code. In tests, `expect("why")` over
  `unwrap()`.
- Never `panic!` except to enforce documented programmer-error invariants.
- Never silently discard errors with `let _ = ...` without a comment.

## Ownership and Borrowing

- Prefer borrowing over cloning; comment any `clone()` in a hot loop (splice
  replay is a hot loop).
- Prefer `&str` over `&String`, `&[T]` over `&Vec<T>` in signatures.
- `Arc<T>` for cross-thread sharing; `Rc<T>` only single-threaded.
- Hold any lock guard in the narrowest scope.

## Types and Data Modelling

- Make illegal states unrepresentable with data-carrying enum variants (e.g. the
  `Version` addressing enum, change-record `op` variants).
- Newtypes over bare primitives where it clarifies intent (a version index, a
  byte offset).
- `Option<T>` over sentinels (`-1`, `""`, `0`).
- Derive `Debug` on every public type; `Clone` / `PartialEq` / `Eq` only when
  meaningful.
- `#[non_exhaustive]` on public enums that may gain variants (the format is a
  0.1 draft, so this applies to record and manifest enums).

## Functions and Structs

- One responsibility per function; aim for ~40 lines, refactor longer into
  helpers.
- No output parameters -- return a value or tuple.
- Constructors: `new` (infallible), `try_new` / `from_*` / `open` (fallible).
- `impl` order: constants -> associated fns -> public methods -> private methods.
- Avoid `pub` fields unless a plain data container; prefer accessors.

## Modules and Visibility

- One concern per file. A `mod.rs` re-exports only, no logic.
- `pub(crate)` for cross-module items that are not public API.
- Keep the `walrus-fmt` public surface small and documented -- it is the
  embeddable library (see "Dependency boundary" in project-conventions).

## Unsafe

- Forbidden except in FFI boundary code (there is none today).
- Every `unsafe` block needs a `// SAFETY:` comment.
- Never use `unsafe` to work around the borrow checker.

## Comments and Docs

- Every public item has a `///` comment describing what and why, not how.
- Use `# Errors`, `# Panics`, `# Examples` where applicable.
- Delete commented-out dead code -- git recovers it.

## Formatting

- `rustfmt` before committing (`cargo fmt`).
- Imports grouped std -> external -> crate-local, blank line between groups.
- Max line length 100.

## Clippy

- Build clean with `cargo clippy -- -D warnings`.
- Suppress a lint only with `#[allow(clippy::lint_name)]` on the smallest scope,
  with a comment explaining why.

---

## Source File Size and Structure

**No source file (`.rs`) should exceed 400 lines**, excluding the `#[cfg(test)]`
module. If non-test code approaches 400 lines, split by concern before adding
more. Splitting is a refactor (no behaviour change).

Why 400: fits one screen with context; forces file-level single-responsibility;
keeps AI-assisted editing reliable; consistent with the ~40-line function guide.

### Comment separators

Section separators use plain ASCII only:
```rust
// === Section Name ===================================================
```
Full character rules for source files are in `documentation.md` (Rust Source
Files section). Replace any non-ASCII separator with ASCII whenever a file is
touched for another reason.
