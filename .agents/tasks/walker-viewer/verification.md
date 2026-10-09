# Verification -- egui 0.36 upgrade + egui_kittest enablement

Worktree: `c:\workspace\Walrus\.worktrees\walker-viewer` (branch `walker-viewer`).
Toolchain: `rustc 1.98.1` / `cargo 1.98.1`. All commands run with the worktree
root as cwd. The reviewer can read this instead of re-running the suites.

## Summary of change

- Workspace `rust-version` 1.75 -> 1.95 (egui 0.36 MSRV), with a comment naming
  the forcing crate. Edition unchanged (2021).
- walker GUI deps: `eframe` / `egui` 0.28 -> 0.36. `rfd` kept at 0.14 (eframe
  0.36 pulls winit 0.30.13; rfd 0.14 resolved and built fine -- no bump needed).
- Added `[dev-dependencies] egui_kittest = "0.36"` (default egui harness, no
  extra features) to walker.
- egui 0.28 -> 0.36 API migration in `crates/walker/src/app.rs`:
  - eframe 0.36 replaced `App::update(&mut self, ctx, frame)` with a non-painting
    `App::logic(&mut self, ctx, frame)` + a painting
    `App::ui(&mut self, ui: &mut egui::Ui, frame)`. Playback time-advance and
    `request_repaint` moved to `logic`; all panel drawing moved to `ui`.
  - egui 0.36 merged the side/top-bottom panel types into a single `Panel`
    struct whose `show` takes `&mut Ui`:
    `egui::TopBottomPanel::top(..).show(ctx, ..)` ->
    `egui::Panel::top(..).show(ui, ..)` (and `bottom`). `CentralPanel::show` also
    now takes `&mut Ui`.
  - `main.rs` needed NO API change (`run_native` 3-arg form and the
    `Ok(Box::new(..))` creator closure are unchanged in 0.36). `main.rs` now uses
    a new thin `src/lib.rs` (re-exports `app` and `player`) so the integration
    test can reach `player::Player`. `player.rs` unchanged (pure logic).
- Added `crates/walker/tests/harness.rs`: one headless `egui_kittest` test.
- Dropped catppuccin-egui from the design (it was never in the worktree
  Cargo.toml). Design + ROADMAP updated.

## Commands run and outcomes

1. `cargo build` (whole workspace)
   -> `Finished dev [unoptimized + debuginfo]`. SUCCESS on the 1.98 toolchain.

2a. `cargo build -p walrus-fmt` (no features)
    -> `Finished dev` in 0.24s. SUCCESS.
2b. `cargo tree -p walrus-fmt`
    -> dependency tree is serde, serde_json, tar, thiserror, zstd (+ dev-dep
    anyhow) only. NO egui / eframe / egui_kittest / any GUI crate. The
    walrus-fmt GUI-free boundary invariant HOLDS.

3. `cargo test -p walker`
   -> 3 player unit tests pass; 1 new harness test
   `step_forward_button_advances_version_index_headless` passes (0.14s, bounded).
   SUCCESS.

4. `cargo test` (whole workspace)
   -> walker: 3 player + 1 harness pass; walrus-fmt: 15 unit tests pass; walx: 0;
   all doc-tests pass. No regressions. SUCCESS.

5a. `cargo fmt -- --check` -> no diff, exit 0. CLEAN.
5b. `cargo clippy -- -D warnings` -> `Finished`, exit 0, no warnings. CLEAN.

(Note on tooling: the shell integration intermittently reported a spurious
non-zero wrapper exit for long/output-heavy cargo runs; each command's real
result was captured by redirecting cargo output to a log file and reading it
back. The outcomes above are the actual cargo exit statuses / test summaries from
those logs.)

## Resolved versions (from Cargo.lock)

| Crate         | Resolved version | Notes                                        |
|---------------|------------------|----------------------------------------------|
| `egui`        | 0.36.2           |                                              |
| `eframe`      | 0.36.0           |                                              |
| `egui_kittest`| 0.36.2           | walker dev-dependency                        |
| `egui_extras` | (absent)         | NOT added -- walker's source does not use it |
| `rfd`         | 0.14.1           | kept at 0.14; no bump needed                 |
| `winit`       | 0.30.13          | pulled transitively by eframe 0.36           |

## Not done here (deferred to the CSV-viewer task)

Design-review findings 3, 5, 6, 7, 8 (export restructuring / MSRV of export
crates / breaking stub signatures / walker-side pulldown-cmark+csv pins) and the
NITs 9, 10 (per-file line budget, XML MIME row) belong to the later CSV-viewer
implementation task, as noted in `design.md` (scope note under section 0 and the
Review-responses tail). This task resolved only the GUI-stack findings 1, 2, 4.
