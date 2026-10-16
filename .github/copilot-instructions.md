# Walrus workspace instructions

The project rules in `.github/instructions/` are the VS Code translations of
the active project steering files in `.kiro/steering/`. Consult the applicable
instruction even for read-only requests when its topic is relevant; not all
instructions are attached in read-only contexts. The Kiro originals are kept
unchanged; keep each VS Code translation synchronized when its source rule
changes.

| Kiro steering source | VS Code instruction |
|---|---|
| `.kiro/steering/project-conventions.md` | `.github/instructions/project-conventions.instructions.md` |
| `.kiro/steering/rust-standards.md` | `.github/instructions/rust-standards.instructions.md` |
| `.kiro/steering/rust-walrus-notes.md` | `.github/instructions/rust-walrus-notes.instructions.md` |
| `.kiro/steering/shell-and-gate.md` | `.github/instructions/shell-and-gate.instructions.md` |
| `.kiro/steering/background-process-status.md` | `.github/instructions/background-process-status.instructions.md` |

When new project steering is added, translate it into the matching VS Code
workspace instruction so it is applied here too.
