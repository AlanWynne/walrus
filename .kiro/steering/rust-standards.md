---
inclusion: always
---

# Rust Coding Standards and File Structure (FFWB)

Workspace-specific. Applies to the FileForgeWorkbench Rust workspace. Universal
process rules live in user space (`workflow.md`, `testing.md`,
`documentation-charset.md`); this file is the Rust-and-FFWB-layout detail.

Apply to every Rust file without being asked. The requirements gate must be
complete before any code is written; these standards apply once it is.

## Naming

| Item | Convention | Example |
|------|-----------|---------|
| Types, traits, enums | `UpperCamelCase` | `DocumentSession` |
| Functions, methods, variables | `snake_case` | `edit_line` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_LINE_WIDTH` |
| Modules | `snake_case` | `file_engine` |

- Prefer full names: `line_number` not `ln`.
- Booleans read as predicates: `is_modified`, `has_pending_changes`.

## Error handling
- Library code: `thiserror`, one `Error` enum per crate with enough context.
- Application/binary code: `anyhow` with `.context("...")` on every `?` crossing
  a module boundary.
- Never `unwrap()`/`expect()` in library code. In tests, `expect("why")` over
  `unwrap()`.
- Never `panic!` except to enforce documented programmer-error invariants.
- Never silently discard errors with `let _ = ...` without a comment.

## Ownership and borrowing
- Prefer borrowing over cloning; comment any `clone()` in a hot loop.
- Prefer `&str` over `&String`, `&[T]` over `&Vec<T>` in signatures.
- `Arc<T>` for cross-thread sharing; `Rc<T>` only single-threaded.
- Hold `Mutex`/`RwLock` guards in the narrowest scope; never across `await` or a
  blocking call.

## Types and data modelling
- Make illegal states unrepresentable -- data-carrying enum variants.
- Newtypes over bare primitives: `struct LineNumber(usize)`.
- `Option<T>` over sentinels (`-1`, `""`, `0`).
- Derive `Debug` on every public type; `Clone`/`PartialEq`/`Eq` only when
  meaningful.
- `#[non_exhaustive]` on public enums that may gain variants.

## Functions and structs
- One responsibility per function; max ~40 lines, refactor longer into helpers.
- No output parameters -- return a value or tuple.
- Constructors: `new` (infallible), `try_new`/`from_*` (fallible).
- `impl` order: constants -> associated fns -> public methods -> private methods.
- Avoid `pub` fields unless a plain data container; prefer accessors.
- One trait per `impl` block.

## Modules and visibility
- One concern per file. `mod.rs` re-exports only, no logic.
- `pub(crate)` for cross-module items that are not public API.
- Max 3 levels deep: `crate::module::submodule`.

## Unsafe
- Forbidden except in FFI boundary code.
- Every `unsafe` block needs a `// SAFETY:` comment.
- Never use `unsafe` to work around the borrow checker.

## Comments and docs
- Every public item has a `///` comment describing what and why, not how.
- Use `# Errors`, `# Panics`, `# Examples` where applicable.
- Delete commented-out dead code -- git recovers it.

## Formatting and clippy
- `rustfmt` before committing. Imports grouped std -> external -> crate-local,
  blank line between groups. Max line length 100.
- Build clean with `cargo clippy -- -D warnings`. Suppress a lint only with
  `#[allow(clippy::lint_name)]` on the smallest scope, with a reason comment.

---

## Source file size and structure

**No source file (`.rs`) may exceed 400 lines**, excluding the `#[cfg(test)]`
module. If non-test code exceeds 400 lines, split before adding more. The split
is a REFACTOR (no gate) provided no observable behaviour changes.

Why 400: fits one screen with context; forces file-level single-responsibility;
keeps AI-assisted editing reliable; consistent with the 40-line function limit.

Split by concern in this order:
1. `_state.rs` -- data structures, `Default` impls, pure data methods
2. `_render.rs` -- all `egui` rendering functions
3. `_commands.rs` -- command dispatch / handler logic
4. `_dialogs.rs` -- modal dialog rendering and state
5. `_tests.rs` -- test module (separate file when it alone exceeds 200 lines)

The primary file becomes a thin coordinator that re-exports and delegates.

### Comment separators (ASCII only, per documentation-charset.md)
```rust
// === Section Name ===================================================
```
