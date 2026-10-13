# Walrus Multi-File Archive and CLI Design

Status: DRAFT -- discussion document, not yet a spec.
Scope: supersedes the earlier single-file framing. Captures the decisions and
open questions from the CLI and GUI design discussion. Covers the multi-file
archive format, the `walrus`/`walx` CLI, and `walker` as a full GUI frontend.
Once settled, the format changes here fold into `FORMAT.md`, the command set into
the `walrus` CLI, and the GUI scope into the walker design.

This document uses ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the repo
documentation convention.

---

## 1. Concept

A Walrus archive is a compressed tar file that bundles a set of tracked files
together with their change history. There are two symmetric variants.

### 1.1 `.wal` (forward log)

A compressed tar archive containing, for each tracked file:

- the ORIGINAL version of the file (its base), and
- a change log for that file.

Reconstruction: `original + change log = current version`.

### 1.2 `.wbl` (reverse log)

A compressed tar archive containing, for each tracked file:

- the CURRENT version of the file (its base), and
- a change log for that file.

Reconstruction: `current - change log = original version`.

The two variants hold the same information from opposite ends. They differ only
in which version is stored as the base and which direction the log replays.

### 1.3 Change from the earlier draft: many files per archive

The earlier specification said a `.wal`/`.wbl` contained exactly one file plus
its log. This is now expanded: an archive holds MANY tracked files, each with
its own base and its own change log. This is the primary format change driving
this discussion. It needs to be reflected in `FORMAT.md` section 2 (tar members
move from a fixed single-file set to a per-file set).

### 1.4 Compression: zstd-only in 0.1, no CLI knobs -- DECIDED

The "compressed" in "compressed tar" is the archive's OUTER codec layer
(FORMAT.md section 2). The decisions for 0.1:

- CODEC IS `zstd`, FIXED. Every 0.1 archive the CLI writes is zstd-compressed.
  gzip is RESERVED in the format (its magic is recognised so a gzip container is
  identified, not mis-read) but no 0.1 writer emits it and the reference reader
  returns a clear "not supported by this build" error rather than decoding it.
- LEVEL IS A WRITER-INTERNAL CONSTANT, not a format property. The reference
  writer uses a fixed zstd level (currently 19 -- small artefact, still fast for
  the small text documents Walrus stores). The level is NOT recorded in the
  archive and a reader never needs it, so changing it later is not a format
  change.
- NO COMPRESSION FLAGS ON THE CLI in 0.1. There is deliberately no `--codec`,
  `--level`, `--compression`, or `--store` on any command. This matches the
  lean-CLI dependency posture (the same reasoning that keeps `--encoding` and
  export out of the CLI, section 4): the user never names a codec; the writer
  picks zstd and the reader sniffs whatever it is given (FORMAT.md section 2,
  codec-from-magic-bytes, INV-5).

WHY THIS IS SAFE TO REVISIT LATER. Adding a better/alternative codec (activating
gzip, or adding xz/lz4/brotli) is an ADDITIVE, backward-compatible change and
needs NO decision now:

- The codec wraps the whole inner tar; manifest, bases, and `.wchg` logs are
  byte-identical regardless of codec, so reconstruction and the byte-identical
  guarantee (FORMAT.md section 5) are untouched by a codec choice.
- Detection is by magic bytes, never the extension, so a new codec adds a magic
  constant plus an encode/decode arm -- no manifest field, no extension change,
  no new library primitive.
- Old archives keep reading unchanged; only an OLD reader facing a NEW-codec
  archive is affected, and it fails CLOSED with the unrecognised-codec error
  (never a silent mis-read).

So the shape to expose compression later -- a `--codec`/`--level` flag on the
mutating commands, driving the same writer -- can be added in a later CLI version
without a format break. It is spec-first per INV-1: register any new codec id +
magic in FORMAT.md section 2 before shipping a writer for it. Nothing in the 0.1
layout needs to change now to keep that door open; it is already open.

---

## 2. Version indexing

- Every recorded change in a file's log has an INDEX.
- The index can be walked to reconstruct the file at any recorded point.
- INDEX 0 is always the CURRENT (latest) version, for both variants.
- Moving backward in time INCREASES the index. Higher index = older version.
- Each tracked file has its own independent index range; the "index size" of a
  file is the number of recorded changes (the highest valid index) for that
  file.

### 2.1 Generation cap (automatic retention)

Each `.wchg` carries a GENERATIONS attribute in its header -- the maximum number
of versions to retain for that file:

- `generations = N` (N >= 1): keep at most N versions. This is a ROLLING WINDOW
  -- when adding a new version would exceed N, the OLDEST version is dropped
  automatically as part of the add/update.
- `generations = 0`: unlimited retention. This is the DEFAULT.

Relationship to condense:
- The generations cap is CONTINUOUS retention, enforced on every add/update.
- `condense --keep <n>` is the MANUAL, one-shot form of the same policy, applied
  on demand.
- Both share the same trimming logic.

IMPORTANT (re-basing on drop): dropping the oldest version is NOT a plain delete.
In a `.wal` the oldest version IS the base (the original); in a `.wbl` the oldest
is the far end of the reverse log. Dropping the oldest therefore RE-BASES: the
next-oldest version becomes the new base and the dropped record is folded in.
This is the same re-base operation condense already performs, so the generations
cap adds no new primitive -- it just triggers the existing trimming
automatically.

CLI exposure: `walrus add`/`walrus update` accept `--generations <n>` to set or
change a file's cap. The long query listing (`-l`) shows each file's generation
cap next to its index size.

### 2.2 Diff engine (producing the splice records)

`add`/`update` must DIFF the incoming file against the current version to produce
the splice record(s) appended to the `.wchg`. The diff is a library concern and
lives in `walrus-fmt`.

DECIDED: do NOT write a diff algorithm. Use an existing, mature pure-Rust engine
behind a thin internal ADAPTER that converts its edit operations into our
byte-offset splice records (`{at, delete_len, insert}`, FORMAT.md section 4.4).

Engine choice:
- PRIMARY: the `similar` crate -- dependency-free, diffs text/bytes at
  line/word/char/grapheme granularity, the de-facto Rust standard (it backs the
  `insta` snapshot library). Dependency-free matters directly for invariant 2:
  it adds essentially no transitive weight to the embeddable library.
- ESCALATION: `imara-diff` -- git-grade (it is the diff engine that powers
  gitoxide; implements git's Histogram algorithm). Swap to it behind the same
  adapter ONLY if profiling shows large-file performance demands it. More
  machinery than `similar`, so not the default.
- REJECTED: writing our own. The Rust engines above are faster and more correct
  than anything hand-rolled; this is not a place to reinvent.

Rationale and constraints:
- Pure Rust, no app/GUI dependency -- the diff + adapter stay inside the GUI-free
  `walrus-fmt` boundary (invariant 2), so both the `walrus` CLI and `walker` get
  the identical capability.
- The engine is SWAPPABLE: only the adapter (ours, small) is coupled to our
  splice format; the algorithm behind it can change without touching the format.
- Diff output is NOT unique -- different algorithms produce different but equally
  valid edit sequences. Our invariant is byte-identical RECONSTRUCTION for a
  given index (FORMAT.md section 5), NOT a minimal or canonical diff. Any correct
  diff whose splices replay to the right bytes is acceptable, so the engine
  choice does not threaten the byte-identical guarantee.

#### 2.2.1 Adaptive granularity, uniform output -- DECIDED

The diff GRANULARITY (how the engine chunks input before comparing) adapts to the
file content; the OUTPUT PRIMITIVE never changes (always byte-offset splices,
invariant 4).

- LINE-ORIENTED content (source code, config -- a reasonable density of line
  terminators): diff at LINE granularity. Smaller, more meaningful records
  ("this line changed" rather than byte churn across a shifted region), and a
  readable `.wchg` history view aligned to lines like `git diff`.
- LONG TEXT WITHOUT line breaks (single-paragraph prose, a one-line JSON blob,
  minified content): line granularity is useless (it is all "one line"), so fall
  back to WORD or CHARACTER granularity.
- CHOOSER: sniff the content -- enough line terminators -> line-oriented; else
  word/char. `similar` supports all these granularities, so this is a config
  choice, not a different engine.
- UNIFORM OUTPUT: whatever granularity is used, the adapter converts the result
  back to byte-offset splices. Line granularity just makes the splices larger and
  better aligned, not a different record type.
- The chosen granularity is recorded as a hint in the `.wchg` header so a
  reader/viewer knows how the splices were generated.

OPEN (ergonomics): auto-detect line-vs-char from content (recommended default)
vs. also exposing an explicit `--diff-granularity <line|word|char>` override on
add/update. Leaning auto-detect with an optional override.

#### 2.2.2 Binary files -- OUT OF SCOPE for now (Option A) -- DECIDED

Walrus tracks TEXT documents only for now. Rationale: the whole format is built
around text (UTF-8 `insert`, declared per-file encoding, splice primitive,
human-readable `.wchg` + viewer). A text diff engine RUNS on binary bytes but
produces useless output -- one small change in a compressed/binary file cascades
into a near-total "replace everything" splice.

- add/update DETECT binary input and REJECT it with a clear error (text is
  required), rather than silently producing garbage splices.
- Binary diffing is a DIFFERENT discipline (delta compression: `bsdiff`/`bspatch`,
  `xdelta`/VCDIFF, `zstd --patch-from`), not a text-diff config. We do not pull
  that in now.

FUTURE EXTENSION (not now): a binary-delta record variant (FORMAT.md section 7
already lists "binary (base64) insert form" as an open question). The record `op`
enum stays extensible (`#[non_exhaustive]` per rust-standards) so a
`binary-delta` op + a `bsdiff`/VCDIFF-class engine can be added additively later,
spec-first per invariant 1, without breaking the format.

---

## 2.3 Internationalization and character encodings -- DECIDED

Walrus is an INTERNATIONAL tool. UTF-8 is the internal storage encoding BECAUSE
it is universal: Unicode (and thus UTF-8) covers every major writing system --
Chinese (simplified and traditional), Japanese, Korean, Arabic, Hebrew, Cyrillic,
Devanagari, emoji. UTF-8 is NOT a limitation; it is the mechanism that makes the
tool work for every language. (The thing that WOULD exclude users is restricting
to ASCII or a single legacy codepage.)

To be international in PRACTICE, not just in theory:

- SOURCE FILES ARE NOT ALWAYS UTF-8. Legacy files may be GB2312 / GBK / GB18030
  (Chinese), Shift-JIS (Japanese), Big5 (traditional Chinese), or the
  windows-125x codepages. These are not UTF-8 bytes.
- ON ADD: detect or accept a DECLARED source encoding, TRANSCODE to UTF-8 on the
  way in, and record the original encoding in the per-file `encoding` manifest
  field (so the field becomes functional, not decorative).
- ON EXTRACT: optionally transcode back to the file's original encoding so the
  user gets the file in the form they expect (a `--encoding`/keep-original
  option; default may restore the recorded original).
- ENGINE: use `encoding_rs` (the pure-Rust encoding library Firefox uses) --
  handles GBK, Shift-JIS, Big5, windows-125x, etc. It fits the GUI-free boundary
  (no app deps), so both CLI and GUI get it.

UNICODE-AWARE DIFF (ties to section 2.2.1). Character-granularity diffing MUST
split on Unicode GRAPHEME CLUSTER boundaries, never raw bytes: a single Chinese
character is multiple bytes in UTF-8, and slicing between them yields garbage.
`similar` supports grapheme-level diffing natively. The splice RECORDS remain
byte-offset (the format primitive is unchanged), but for text the chosen split
points -- and therefore the byte offsets -- always land on character boundaries,
never mid-character.

---

## 3. Naming: archive extension vs. inner per-file log extension

Problem: the current design reuses `.wal`/`.wbl` for both the outer archive and
the inner per-file change log. That is confusing.

Decision:

- The ARCHIVE (the compressed tar the user handles on the command line) keeps
  `.wal` / `.wbl`. The extension signals the variant at a glance. Linux does not
  require extensions, but the convention is kept deliberately for readability.
- The INNER per-file change log uses the extension `.wchg` ("walrus change
  log"), DISTINCT from the archive so the two are never confused.
- The `.wchg` extension is the same for both variants, BUT each `.wchg` carries
  a DIRECTION ATTRIBUTE in its own header marking it as a forward or reverse log.
  This makes an extracted `.wchg` self-describing and usable OUTSIDE the archive:
  a reader does not need the parent archive's extension to know how to replay the
  log. (So the extension is variant-neutral while the file content is not.)

Alternatives considered: `.walog`/`.wblog` (encodes direction in the extension
-- rejected in favor of a header attribute, which is more robust and keeps one
extension).

---

## 4. CLI conventions (POSIX / GNU)

The CLI follows standard POSIX/GNU conventions:

- Short options: single dash + single letter (`-x`, `-q`, `-a`, `-u`, `-r`).
- Long options: double dash (`--extract`, `--query`, ...).
- An option that takes a value: `--extract 4`, `--extract=4`, `-x 4`, `-x4`.
- `--` terminates option parsing, so paths that look like options still work.
- Operations (query / extract / add / update / remove / condense) are MUTUALLY
  EXCLUSIVE. Supplying more than one is a usage error (RC 2, see table).
- stdout carries data; stderr carries diagnostics and warnings.

DECIDED: external CLI follows POSIX / GNU parameter conventions (above).

CLI SHAPE -- DECIDED: SUBCOMMANDS.

The canonical form is `walrus <op> <archive> [mask] [options]`, like `git` and
the current `walx`. This gives cleaner per-operation help, natural option
grouping, and avoids an operation hiding as a flag after positional arguments.

```
walrus query    <archive> [file-mask] [-l]
walrus extract  <archive> [file-mask] [--index <n>] [-o <dir>] [--force] [--sanitize]
walrus cat      <archive> <file>       [--index <n>]
walrus add      <archive> <files...>  [--generations <n>] [--wal | --wbl | --direction <forward|reverse>] [-R]
walrus update   <archive> <files...>  [--generations <n>] [-R]
walrus remove   <archive> <file-mask> [--force]
walrus condense <archive> <file-mask> [--keep <n>]
walrus convert  <archive> [file-mask] --to <wal|wbl> [-o <name>]
```

The flag spellings (`-x`/`--extract`, `-q`/`--query`, `-a`/`--add`, `-u`/
`--update`, `-r`/`--remove`, `--condense`) are retained as convenience aliases,
but the subcommand is canonical. The per-command examples in section 5 show both
the subcommand form and the alias form.

SCOPE BOUNDARY -- DECIDED: NO EXPORT IN THE CLI.

The `walrus` CLI does NOT transform content to other presentation formats
(txt/md -> html, docx, pdf, ...). There is NO `export` subcommand and no
export-* cargo feature on the CLI binary. Format TRANSFORMATION is exclusively
`walker`'s (the GUI's) domain.

Consequences:
- The CLI reconstructs and emits a tracked file in its OWN stored format only:
  `extract` restores the stored bytes to disk (section 5.2.1); `cat` writes the
  stored bytes to stdout (section 5.2.2). Neither converts the content type.
- The transformer backends (the `export/` module -- csv/docx/html/json/markdown/
  pdf) and their heavy dependencies (`docx-rs`, `printpdf`, `pulldown-cmark`,
  `scraper`, ...) are used ONLY by `walker`. They may live behind `walrus-fmt`
  optional cargo features (per project-conventions invariant 2), but the CLI
  never enables them. A playback/management-only user (and the CLI) stays lean.
- `.wal <-> .wbl` CONVERSION (`walrus convert`) is NOT an "export": it changes
  the archive VARIANT, not the content presentation. It stays in the CLI.

This keeps `walrus-fmt` GUI-free and keeps the export-heavy dependency surface
out of the base CLI, exactly as the dependency boundary (invariant 2) requires.

BINARY NAME -- DECIDED: `walrus`.

The full management tool is named `walrus`. The short name `wal` was rejected
because it COLLIDES with existing, actively-used tools:

- `wal` (pywal) -- generates terminal colorschemes from wallpaper images; ships a
  binary literally named `wal`, widely installed in the Linux theming community.
- `wal-lang` (Waveform Analysis Language) -- a hardware-waveform DSL on PyPI.
- `wall` (util-linux write-to-all) -- one letter off, easy to fat-finger.

A user with pywal installed would hit a `$PATH` collision, whichever binary
comes first silently shadowing the other. This is the same reasoning that pushed
the library to publish as `walrus-fmt` rather than `walrus`.

`walrus` was chosen because it matches the project name and has NO realistic
`$PATH` collision: no mainstream distro package ships a `/usr/bin/walrus`.

Known non-blocking caveat: `walrus` is a popular project NAME across unrelated
domains -- a Sui blockchain blob-storage CLI, a Ruby templating tool, a backup
system, a WASM runtime, a log-streaming engine, a git query tool. None of these
install a bare `walrus` binary on a typical user's default path, so the
practical collision risk is low, but the name is not unique. `walr` was
considered (more unique) but sits awkwardly between the theming tools `walrs` and
`wallr`.

DECIDED: the full tool is `walrus`; `walx` remains the extract-only tool.
Reconciling the published library name (`walrus-fmt`) with a `walrus` binary is a
packaging detail (the binary and library namespaces are separate), not a blocker.

DECIDED (walx scope): `walx` mirrors the extract/cat split as its extract-only
surface -- it provides `extract` (restore to disk, section 5.2.1) and `cat`
(bytes to stdout, section 5.2.2) and nothing that mutates an archive. The full
management set (query/add/update/remove/condense/convert) is `walrus`-only.

#### `wal` as an OPT-IN, collision-aware alias -- DECIDED

Rejecting `wal` as the CANONICAL binary name (above) does NOT forbid `wal` as an
optional alias. The two are different commitments: the canonical name is what the
installer ALWAYS puts on `$PATH` (and must never silently shadow pywal's `wal`),
whereas an alias is something the user deliberately opts into when their `$PATH`
is clear. So `walrus` remains the one guaranteed binary, and `wal` is offered as
a convenience the user turns on at their own discretion -- "usable as long as the
wallpaper tool (pywal) is not in the way", exactly the intended framing.

LOAD-BEARING RULE: nothing in a default install may create a `wal` entry on
`$PATH`. No second `[[bin]] name = "wal"`, no post-install script that drops a
`wal` unconditionally. Doing so would reintroduce the precise silent-shadow
problem that got `wal` rejected -- it would merely move the collision from
"always" to "on install", which is still default-on. The alias is ALWAYS opt-in
and ALWAYS collision-checked before creation. This mirrors how `ripgrep` ships
`rg`, and how Debian ships `batcat`/`fdfind` and documents a user-added short
alias rather than forcing one.

Two mechanisms, both opt-in:

1. DOCUMENTED SHELL ALIAS (zero-risk, always available). The README / install
   notes tell the user how to add the alias themselves:
   - POSIX shells: `alias wal=walrus` in the shell rc.
   - PowerShell: `Set-Alias wal walrus` (or a thin function) in the profile.
   A user-defined shell alias is resolved by the shell before `$PATH`, is trivial
   to remove, and can never clobber another binary on disk. This alone satisfies
   the request for most users and needs no code.

2. `walrus alias` HELPER SUBCOMMAND (opt-in, collision-aware). For users who want
   a real `wal` on `$PATH` (so it works from scripts and other programs, not just
   an interactive shell), `walrus` provides a guarded installer:

   ```
   walrus alias install [--name <alias>] [--dir <dir>] [--force]
   walrus alias remove  [--name <alias>] [--dir <dir>]
   walrus alias status  [--name <alias>]
   ```

   - DEFAULT ALIAS NAME is `wal`; `--name` allows a different short name.
   - `install` resolves a target directory (`--dir`, else the first user-writable
     `$PATH` entry, e.g. `~/.local/bin` on POSIX or the user bin dir on Windows)
     and creates a shim pointing at the real `walrus` binary: a SYMLINK on POSIX,
     and on Windows a small shim (a `.cmd`/`.exe` launcher; a symlink only if the
     environment permits it without elevation).
   - COLLISION CHECK IS MANDATORY. Before creating anything, `install` searches
     `$PATH` for an existing `wal`. If one is found that is NOT our own shim (e.g.
     pywal), it REFUSES: prints what owns `wal` (the resolved path) and exits
     RC 2, creating nothing -- UNLESS `--force` is given, in which case it still
     refuses to overwrite a file it did not create and instead instructs the user
     to pick `--name` or `--dir` (we never delete or shadow a foreign binary).
     An existing `wal` that IS our own prior shim is simply re-pointed (idempotent).
   - `remove` deletes ONLY a shim we created (verified by target), never a foreign
     `wal`; a no-op with a note if none of ours is present.
   - `status` reports whether a `wal` is on `$PATH`, its resolved target, and
     whether it is our shim, pywal, or something else -- so the user can decide
     before installing.
   - RETURN CODES: refuse-on-foreign-collision -> RC 2 (usage/safety, matching the
     refuse-by-default convention elsewhere); success -> 0; `remove` with nothing
     of ours present -> 0 with a stderr note.

   The helper does NOT modify shell rc files or the registry; it only manages a
   single shim file in a bin directory, so `remove` fully reverses `install`.

RATIONALE: this gives the short, ergonomic `wal` to users who want it, on both
the interactive-shell path (mechanism 1) and the real-`$PATH` path (mechanism 2),
without the canonical install ever endangering a pywal user. The canonical-name
DECIDED above is unchanged; this only adds an opt-in layer on top of it.

OPEN (not blocking 0.1): distro-level integration via `update-alternatives`
(Linux) so a packager can register `wal` as a managed alternative the user
selects -- packaging-maintainer territory, deferred past the 0.1 CLI. The
`walrus alias` helper above is the cross-platform, no-package-manager answer in
the meantime.

---

## 5. Commands

Shared terms:

- `<archive>` -- a `.wal` or `.wbl` file. Every operation works identically on
  both variants.
- `<file-mask>` -- a single name, a list of names, or wildcard pattern(s). A mask
  that matches multiple tracked files applies the operation to each match.

### 5.1 Query (list contents) -- default

```
walrus query <archive>                 # canonical
walrus query <archive> <file-mask>
walrus query <archive> <file-mask> -l  # long listing
walrus <archive>                       # alias: bare archive -> query
walrus <archive> -q
```

- Query is the default when no operation is given.
- Produces a directory listing (an `ls` / `dir`) of the tracked files.
- Two listing styles, mirroring `ls`:
  - terse (default): one file name per line, plus its index size.
  - long (`-l` / `--long`): file attributes (size, mtime, permissions) PLUS the
    index size (number of recorded versions) for each file.
- With a `file-mask`, only matching files are listed.
- Reports for each file: the tracked file name and attributes, and its index
  size.

DECIDED: "index size" = the COUNT of recorded versions (the highest valid index)
for the file. The long listing (`-l`) additionally shows a "log bytes" column
(the byte size of the inner `.wchg` log) alongside the file attributes -- this is
the "attributes + log bytes" the requirements call for (FR-W11 AC1), and it is
the same `log_bytes` figure the Walker bloat indicator uses (section 6b.3).

### 5.2 Extract (restore to disk) and Cat (write to stdout)

These are TWO distinct operations, deliberately separated so each matches the
convention its name implies. The earlier draft defined a single `extract` that
wrote to stdout; that was the old `walx` behavior and is corrected here.

#### 5.2.1 `extract` -- restore file(s) to disk

```
walrus extract <archive> [file-mask] [--index <n>] [-o <dir>] [--force]
walrus <archive> [file-mask] -x                              # alias
```

- Matches the archive-tool convention (`tar -x`, `unzip`, `7z x`): reconstructs
  the selected file(s) at the requested version and WRITES THEM TO DISK under
  their stored names, recreating any relative path from the manifest and
  restoring recorded attributes.
- `file-mask` optional: omitted -> ALL tracked files; wildcards select a subset.
- VERSION SELECTOR (optional, applies to EVERY selected file, resolved PER FILE
  against that file's own `[0, count]` range -- see FORMAT.md section 5). The
  selectors below mirror the library `Version` enum (FR-W3) so the CLI is as
  expressive as the engine beneath it. At most one selector may be given
  (supplying two is RC 2):
  - omitted (or `--version current`) -> index 0, the CURRENT version. Default.
  - `--version original` -> each file's OLDEST version (its own `count`). This is
    how you extract the oldest version of EVERY file at once; because it resolves
    per file it is correct for a mixed-history archive, and it is a clean success
    (RC 0, no warning) -- unlike a numeric index past the end.
  - `--index <n>` -> that numeric index for every file. Greater than a file's
    last recorded index -> the ORIGINAL is written for THAT file with a WARNING,
    and the command exits RC 4 (the clamp-and-warn safety fallback; it applies
    only to an out-of-range NUMERIC index, never to `original`/`current`/`--at`).
  - `--version <+/-n>` -> RELATIVE to current (e.g. `-1` = one older than
    current), clamped into the file's range.
  - `--at <T>` -> SNAPSHOT AT A TIMESTAMP. For each file, the version whose change
    record is the newest at-or-before `T`; a `T` before a file's earliest record
    resolves to that file's current version (index 0). `T` is a UTC ISO-8601
    instant in the fixed shape `YYYY-MM-DDThh:mm:ssZ` (the trailing `Z` is
    required; see the UTC rule below). This reconstructs the whole archive as it
    stood at an instant -- a point-in-time snapshot across all files.
  - `--label <name>` -- resolve by a file's checkpoint label (FORMAT.md 5.2).
    OPEN (deferred past 0.1): checkpoint labels are a thinner, less-exercised
    path; `--label` ships once checkpoint authoring is exposed. `--at`,
    `original`, `current`, numeric, and relative all ship in 0.1.
  `--index <n>` is retained as the explicit numeric spelling; `--version` accepts
  `current` / `original` / a signed relative integer; `--at` is its own flag.
- `-o` / `--output <dir>`: destination directory. DEFAULT is the current working
  directory. Stored relative paths are recreated beneath it.
- DECIDED (overwrite safety): if a target path ALREADY EXISTS, extract REFUSES
  that file and exits RC 2, unless `--force` is given (then it overwrites).
  Consistent with the remove-safety rule (section 5.5).

#### 5.2.2 `cat` -- write ONE file's bytes to stdout

```
walrus cat <archive> <file> [--index <n> | --version <current|original|+/-n> | --at <T>]
```

- The `cat` (Linux) / `type` (Windows) analogue: reconstructs ONE tracked file at
  the requested version and writes its raw bytes to STDOUT, so the user pipes or
  redirects it wherever they want. This is what the old `extract` did.
- Exactly ONE tracked file must be selected (not a multi-match mask); selecting
  zero or more than one is a usage error (RC 2).
- VERSION SELECTOR: the SAME selector set as extract (section 5.2.1) --
  `--index <n>`, `--version current|original|<+/-n>`, and `--at <T>` (UTC
  snapshot), resolved against the single selected file's range; omitted ->
  current; a numeric index beyond-last -> original + warning + RC 4; `--label`
  deferred as in extract. At most one selector (two is RC 2).
- No `-o`, no `--force`: `cat` never touches disk. Redirection is the user's job:
  `walrus cat a.wal notes.txt > just-this.txt` or `walrus cat a.wal notes.txt |
  grep TODO`.
- NAME CHOICE: the subcommand is `cat` on all platforms (no `type`, which
  collides with the cmd.exe builtin and reads oddly as a subcommand). As a
  subcommand `walrus cat` has no builtin collision.

Rationale for the split: "extract" universally means restore-to-disk; a separate
`cat` is the stdout path. Two names because they genuinely do different things
(disk vs stdout, multi-file vs single-file), so `cat` is a real subcommand, not a
mere alias of extract.

### 5.3 Add

```
walrus add <archive> <files-to-add...> [--generations <n>] [-R | --recurse]   # canonical
walrus add <archive> - --name <stored-name> [--generations <n>]                # from stdin (5.4e)
walrus <archive> <files-to-add...> -a                                          # alias
```

- `files-to-add` may be a list of names/paths, including wildcards.
- A single `-` operand reads the content from STDIN instead of a file and
  REQUIRES `--name <stored-name>` to supply the archive name (section 5.4e).
- For each file: it becomes the new current version; a diff against the archive's
  existing current version is computed and the change log updated.
- Works whether or not the file already exists in the archive. If it does not
  exist, it is created as a new tracked file (its first version becomes both base
  and current).
- `--generations <n>` sets the file's generation cap (see section 2.1). Omitting
  it on an existing file leaves the current cap unchanged; on a new file it
  defaults to 0 (unlimited).
- If adding exceeds a file's generation cap, the oldest version(s) are
  auto-dropped (re-base) per section 2.1.
- `-R` / `--recurse` descends into any directory argument and adds every file
  found beneath it, storing each under its path RELATIVE to the recursion root
  as the archive name (section 5.4b). Without `-R`, a directory argument is a
  usage error (RC 2).
- A file-input operand that contains wildcard metacharacters is expanded by
  walrus itself before any file is read (section 5.4f). This is what makes
  `walrus add base.wal *.md` behave identically on Windows and POSIX.

#### Direction flag `--wal` / `--wbl` / `--direction` -- DECIDED

`add` accepts `--wal` / `--wbl` (equivalently `--direction <forward|reverse>`,
where `--wal` == `--direction forward` and `--wbl` == `--direction reverse`) to
choose a file's replay direction. Per-file direction is IMMUTABLE once a file is
tracked (only `convert`, section 5.7, flips an existing file), so the flag only
ever applies at CREATE time:

- CREATE-ONLY OVERRIDE: the flag sets the direction for a file being CREATED by
  this `add`. For a file ALREADY tracked in the archive, the flag is IGNORED and
  the existing direction kept (changing it would be a `convert`, not an `add`).
- BRAND-NEW ARCHIVE (no manifest yet) SEED ORDER for the file's direction:
  1. the explicit flag (`--wal`/`--wbl`/`--direction`), else
  2. the archive filename extension (`.wal` -> forward, `.wbl` -> reverse), else
  3. the default `wal` / forward for a bare archive name with no extension.
- EXISTING ARCHIVE: the manifest `variant` drives the default direction for a
  newly added file; the archive filename EXTENSION is NOT consulted. This keeps
  behaviour identical on extension-blind Unix systems, where the manifest -- not
  the name -- is authoritative.

This is the CLI realisation of FORMAT.md's rule that the archive variant seeds
the default direction for newly added files while each file's own `.wchg`
`direction` remains authoritative for reconstruction (section 6a).

OPEN (deferred past 0.1): a source-encoding flag. FR-W8 / section 2.3 decide the
i18n TRANSCODE behaviour (accept a declared non-UTF-8 source, transcode to UTF-8
on `add` via `encoding_rs`, record the original encoding per file, optionally
transcode back on `extract`), but that is a `walrus-fmt` LIBRARY capability
behind the optional `i18n` cargo feature. The 0.1 CLI is deliberately UTF-8-only
and does NOT expose an `--encoding <enc>` flag on `add` (nor `--encoding` /
`--keep-original` on `extract`): adding it would pull `encoding_rs` into the CLI
binary, which cuts against the lean-CLI dependency boundary (the same reason
export stays Walker-only, section 4). Non-UTF-8 handling is available to Walker
via the library today; the CLI flags are additive and can be turned on in a later
version without a format change. (A non-UTF-8 file given to the 0.1 CLI is
rejected as binary per section 2.2.2 / the `BinaryInput` error.)

### 5.4 Update

```
walrus update <archive> <files-to-update...> [--generations <n>] [-R | --recurse]   # canonical
walrus update <archive> - --name <stored-name> [--generations <n>]                   # from stdin (5.4e)
walrus <archive> <files-to-update...> -u                                             # alias
```

- A single `-` operand reads the content from STDIN instead of a file and
  REQUIRES `--name <stored-name>` to name the target (section 5.4e); the named
  file must already be tracked or the command fails RC 8, as for any `update`.
- If the file EXISTS in the archive: it becomes the new current version; a diff
  against the current version is computed and the change log updated.
- If the file DOES NOT exist: report that it must be added instead; fail RC 8
  (the library's `NotTracked` error, section 6).
- `--generations <n>` sets the file's generation cap (see section 2.1); omitting
  it leaves the cap unchanged.
- If updating exceeds a file's generation cap, the oldest version(s) are
  auto-dropped (re-base) per section 2.1.
- `-R` / `--recurse` descends into any directory argument and updates every file
  found beneath it, matching each by its path RELATIVE to the recursion root
  against the archive name (section 5.4b). Because update refuses files absent
  from the archive, a recursed file not already tracked fails RC 8 per the batch
  transaction (section 5.4a); use `add` to introduce new files. Without `-R`, a
  directory argument is a usage error (RC 2).

Difference from Add: Add creates a missing file; Update refuses and fails. The
diff/update behavior is otherwise identical.

- A file-input operand that contains wildcard metacharacters is expanded by
  walrus itself before any file is read (section 5.4f), exactly as for `add`.

#### No-change save (diff finds nothing) -- DECIDED

When `add` or `update` is given content byte-identical to the file's CURRENT
version, the diff produces no net splice. This is a SUCCESS no-op, not an error:
the file is left exactly as it was, NO new version is recorded, and the version
index does NOT advance (it counts real saves, consistent with the one-net-splice-
per-save rule, FORMAT.md 4.4; the library already returns success and stores
nothing in this case). This mirrors the familiar "nothing to commit" of a VCS.

- RETURN CODE: 0 (success). A no-change save is the archive already being in the
  desired state, not a failure.
- REPORTING: the CLI prints a per-file note to STDERR (e.g. `doc.txt: no change,
  not versioned`) so a no-op is never mistaken for a recorded version, while
  STDOUT stays clean for scripting.
- BATCH / RECURSE: in a multi-file or `-R` add/update, each file is judged
  independently -- changed files version normally, unchanged files are skipped
  with the note above, and the command still exits 0. A no-change file is NOT a
  batch failure and does NOT trigger the transactional abort (5.4a); only a genuine
  error (a binary file per 2.2.2, or an `update` target absent from the archive,
  RC 8) aborts the batch and leaves the archive byte-for-byte unchanged.
- `--generations <n>` ON A NO-CHANGE SAVE: the content stores nothing, but an
  explicitly supplied generation cap is still applied to the file (and may trigger
  a re-base/trim if the existing count now exceeds it), since setting the cap is a
  separate intent from recording a new version. Omitting `--generations` leaves
  the cap untouched, as stated above.

### 5.4a Transactional (all-or-nothing) writes -- DECIDED

Any operation that mutates the archive (add, update, remove, condense) is
TRANSACTIONAL across the whole batch:

- The new archive is built to a TEMP file, then ATOMICALLY RENAMED over the
  original only after EVERY file in the batch succeeds.
- If any file in the batch fails, the temp file is discarded and the original
  archive is left BYTE-FOR-BYTE UNCHANGED.
- Rationale: a partial write that fails mid-way through rewriting the tar could
  leave a truncated or inconsistent (corrupt) archive. Temp-file-plus-atomic-
  rename gives all-or-nothing with no corruption window.
- On failure the command exits non-zero (RC 8 when a target does not qualify,
  otherwise a general failure code).

### 5.4b Directory recursion (`-R` / `--recurse`) -- DECIDED

`add` and `update` take file arguments, not directories. `-R` / `--recurse`
extends them to descend into directory arguments and ingest the files beneath.

- FLAG SPELLING: `-R` (uppercase) or `--recurse`. Uppercase deliberately, because
  the lowercase short `-r` is already the `remove` alias (section 5.5); reusing it
  for recursion would be ambiguous. Uppercase `-R` also matches the common
  recursive-flag convention (`cp -R`, `grep -R`, `ls -R`).

- WITHOUT `-R`: a directory given as a file argument is a usage error (RC 2) --
  add/update operate on files, and silently skipping a directory would hide a
  mistake.

- WITH `-R`: each argument is classified:
  - a FILE argument is ingested as before (its name is the stored archive name,
    exactly as in the non-recursive case -- `-R` does not change how plain file
    arguments are named);
  - a DIRECTORY argument becomes the RECURSION ROOT: the directory is walked and
    every regular file found beneath it (at any depth) is ingested.

- STORED NAME = RELATIVE PATH. A file discovered under a recursed directory is
  stored under its path RELATIVE to the recursion root, with the root directory's
  own name as the leading component, using `/` as the separator on every
  platform (the manifest `name` field already permits `/`, section 2 /
  manifest.rs). Example: `walrus add a.wal src -R` where `src/` contains
  `main.rs` and `net/http.rs` stores the archive names `src/main.rs` and
  `src/net/http.rs`. This makes the relative tree round-trip: `extract`
  (section 5.2.1) already recreates stored relative paths on disk, so a recursed
  add followed by an extract reproduces the directory layout.

- MASK INTERACTION: a wildcard file-input operand expands to a set of paths
  FIRST -- by the shell on POSIX, else by walrus's own portable glob (section
  5.4f) -- and `-R` then governs only how a DIRECTORY among those resolved paths
  is treated (descend vs. RC 2). A pattern that matches only files behaves
  identically with or without `-R`.

- SYMLINKS AND SPECIAL FILES: the walk follows regular files only. Symbolic links
  are NOT followed (avoids cycles and escaping the recursion root); directory
  symlinks and non-regular files (devices, sockets, FIFOs) are skipped. A skipped
  entry is reported on stderr but does not fail the batch.

- BINARY FILES: recursion does not relax the text-only rule (section 2.2.2). A
  binary file encountered during the walk is rejected like any other binary
  input; under the batch transaction (section 5.4a) that aborts the whole add/
  update and leaves the archive unchanged. (A future `--skip-binary` ergonomics
  option is noted as an open question, not decided here.)

- DETERMINISM: directory entries are ingested in a STABLE sorted order (by
  relative path) so a given directory tree always produces the same batch order
  and the same resulting archive, satisfying the deterministic-output rule in the
  testing convention.

- EMPTY DIRECTORIES carry no file and are simply not represented (the archive
  tracks files, not directory nodes).

OPEN (ergonomics, not blocking): `--skip-binary` to skip rather than reject binary
files during a recurse; include/exclude globs layered on top of the walk. Both are
additive and can be decided when the add/update implementation lands.

### 5.4c Cross-platform names and mask semantics -- DECIDED

Recursion (5.4b) stores a RELATIVE PATH as the archive `name`. Those names must
round-trip between platforms (an archive built on Windows must extract to the
identical tree on Linux and vice versa), so the separator handling and the
`<file-mask>` matching are pinned here. These rules apply to every command that
reads or matches a stored name (`query`, `extract`, `cat`, `remove`), not just
`add`/`update`.

- CANONICAL STORED SEPARATOR IS `/`, ALWAYS. A stored `name` uses `/` as its one
  and only path separator on every platform (restating the 5.4b invariant). The
  archive is thus platform-neutral: it never contains a `\` as a separator.

- NORMALIZE ON WRITE (add/update, the producing side). When deriving a stored
  name from an on-disk relative path, convert the running platform's OS path
  separator to `/`. On Windows that means `\` (the OS separator) becomes `/`; the
  name `src\net\http.rs` is stored as `src/net/http.rs`. On a POSIX platform the
  separator is already `/` and NO backslash substitution is performed -- a `\` in
  a POSIX filename is a legal DATA character and is preserved verbatim (it is not
  a separator there). So the rule is "normalize the OS's actual separator", NOT
  "replace all backslashes"; the latter would corrupt a POSIX file literally
  named `a\b.txt`.

- LOCALIZE ON READ (extract, the restoring side). Extract splits a stored name on
  `/` and rebuilds the on-disk path with the LOCAL platform separator (identity
  on POSIX; `/` -> `\` on Windows). Intermediate directories are created as
  needed (mkdir -p semantics). Extract MUST NOT hand a raw stored string to the
  OS and rely on incidental acceptance; it splits on `/` and joins components.

- MASK MATCHING IS SEGMENT-AWARE OVER `/`. A `<file-mask>` is matched against the
  stored `/`-separated name with these glob semantics (one rule shared by
  `query`, `extract`, `remove`):
  - `*` matches within a single path segment and does NOT cross `/`
    (so `src/*` matches `src/main.rs` but NOT `src/net/http.rs`);
  - `**` matches across segments including `/`
    (so `src/**` matches both `src/main.rs` and `src/net/http.rs`);
  - `?` matches a single non-`/` character;
  - a mask with no wildcard is an exact whole-name match.
  Matching is performed on the normalized `/` form, so the SAME mask selects the
  SAME files regardless of the platform the command runs on.

  SCOPE OF THIS MATCHER vs. ADD/UPDATE INPUT GLOBBING. This internal matcher
  operates on STORED names and governs `query` / `extract` / `remove` ONLY. It
  is a SEPARATE mechanism from the filesystem glob that `add` / `update` apply
  to their FILE-INPUT operands (section 5.4f): the former selects already-tracked
  names inside the archive, the latter expands a pattern against the working
  directory to find files to ingest. A shell may also pre-expand an unquoted
  wildcard before walrus sees it; on POSIX it usually does, and walrus then
  receives literal paths. The add/update input globber (5.4f) is deliberately
  LITERAL-PATH-WINS so a shell-expanded pattern is never re-globbed, and so the
  natural Windows invocation (where the shell does NOT expand) still works. The
  two mechanisms share the same `*` / `**` / `?` segment semantics but never run
  on the same input.

### 5.4d Extract naming safety: reserved names, path escaping, sanitize, rename -- DECIDED

A stored name is platform-neutral, but the TARGET filesystem may be unable to
represent it: Windows reserves characters (`< > : " | ? *`) and device names
(`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, with or without an
extension), forbids trailing dots/spaces, and is case-insensitive; a crafted or
cross-platform archive may also carry a name that escapes the output directory.
Extract's policy, consistent with the fidelity-first, refuse-by-default posture
already used for overwrite safety (5.2.1):

- SECURITY -- PATH ESCAPING IS ALWAYS REFUSED (not optional, no flag relaxes it).
  Before writing, extract rejects any stored name that, once resolved under
  `-o <dir>`, would land OUTSIDE `-o <dir>`: an absolute path, a leading `/`, a
  drive-qualified path, or any `..` component. Such a file is refused with a
  stderr message and the command exits RC 2. This closes the "zip-slip" class of
  vulnerability and holds even with `--sanitize` or `--force`.

- DEFAULT -- REFUSE AND REPORT (per file). A target that the local filesystem
  cannot represent (reserved device name, illegal character, trailing dot/space,
  or a case-collision with a file already written this run) is REFUSED for that
  file, reported on stderr naming the file and the reason, and the command exits
  RC 2. Other selected files still extract: extract writes to disk per file and
  is NOT transactional (unlike the mutate ops in 5.4a), so one unrepresentable
  file does not abort the batch. No silent mangling by default -- a renamed file
  would no longer match its stored name and would break the add/extract
  round-trip invariant.

- OPT-IN -- `--sanitize` (bulk convenience, the 7-Zip-style path). When the user
  wants the bytes out and does not need names preserved, `--sanitize` deterministic-
  ally rewrites an unrepresentable target so it can be written: reserved characters
  -> `_`, a reserved device name gets a `_` suffix (`aux` -> `aux_`), trailing dots/
  spaces trimmed. Each remapping is reported on stderr (stored name -> written
  name) so the result is auditable. `--sanitize` never relaxes the path-escaping
  refusal above, and still refuses (RC 2) if a sanitized name collides with an
  already-written file unless `--force` is also given.

- SINGLE-FILE RENAME IS AVAILABLE TODAY VIA `cat`. To restore ONE file under a
  chosen name, `cat` already writes raw bytes to stdout with redirection as the
  rename mechanism (5.2.2): `walrus cat a.wal "weird:name.txt" > safe.txt`. This
  is the supported single-file rename path and needs no new flag. (It does not
  restore recorded attributes or create directories -- those are extract's job.)

- RETURN CODES: path-escape or unrepresentable-name refusal -> RC 2 (usage/safety),
  matching the existing overwrite-refusal code; the beyond-last-index RC 4 and the
  target-missing RC 8 semantics from 5.2.1 / section 5 are unchanged.

OPEN (ergonomics, not blocking): an explicit extract rename flag `--as <name>`
(valid only when the mask selects exactly one file, same single-match rule as
`cat`) to restore that one file under a given name WITH attribute restoration and
`-o`/`--force` -- the capability `cat` cannot provide. Deferred unless real use
demands it; `cat`-with-redirect covers single-file rename in the meantime.

### 5.4e Reading content from stdin (`-` + `--name`) -- DECIDED

`add` and `update` take file-path arguments and derive each tracked file's
archive `name` from its path (section 5.4b/5.4c). A piped stream has no path, so
stdin ingestion needs an explicit source marker and an explicit name. This
matches the universal Unix convention (`tar`, `cat`, `curl -T -`) and reuses the
existing POSIX `-` idiom and `--` option terminator (section 4); it adds no
format change (INV-1) and no new library primitive -- the buffered bytes funnel
through the same `Archive::add` / `Archive::update` path as a file argument.

- SOURCE MARKER `-`: a single `-` in the file-operand position means "read the
  content from STDIN". The CLI reads stdin to EOF into a buffer, then proceeds
  exactly as if that buffer were a file's bytes.

      some-command | walrus add    <archive> - --name notes.txt
      some-command | walrus update <archive> - --name notes.txt

- `--name <stored-name>` IS REQUIRED WITH `-`. There is no path to derive from,
  and the archive is multi-file, so every tracked file needs a name. A `-` source
  WITHOUT `--name` is a usage error (RC 2). The value becomes the manifest `name`
  VERBATIM: it uses `/` as its only separator (section 5.4c), and is subject to
  the same path-escaping and reserved-name rules that govern any stored name
  (section 5.4d) -- an escaping or illegal `--name` is refused RC 2.
  (`--name` is meaningful ONLY with a `-` source; supplying it alongside file
  arguments is a usage error, RC 2.)

- EXACTLY ONE STREAM. stdin is a single stream: `-` may appear at most once, and
  `-` may NOT be mixed with file-path arguments in the same invocation (RC 2).
  `-R` / `--recurse` is meaningless with `-` (there is no directory to walk) and
  is a usage error (RC 2).

- TEXT-ONLY, SAME AS A FILE. The buffered stdin bytes are subject to the
  text-only rule (section 2.2.2): non-text / invalid-UTF-8 / NUL-bearing input is
  rejected with the `BinaryInput` error, exactly as a binary file argument would
  be. The 0.1 CLI is UTF-8-only (no `--encoding`, section 5.3), so a non-UTF-8
  stream is rejected; piping arbitrary binary (e.g. `tar c | walrus add`) is not
  supported in 0.1 by design.

- DIRECTION / GENERATIONS / TRANSACTIONS UNCHANGED. `--generations <n>` and the
  create-time direction flags (`--wal`/`--wbl`/`--direction`, section 5.3) apply
  to a stdin `add` identically to a file `add`; the write is transactional
  (section 5.4a). `update` from stdin obeys the same "must add first" rule
  (RC 8, section 5.4) when `--name` is not already tracked.

- NO-CHANGE SAVE. A stdin stream byte-identical to the named file's current
  version is the success no-op of section 5.4 (RC 0, stderr note, no new
  version), exactly as for a file argument.

RATIONALE: the only genuinely new decisions are the source marker and the
name -- everything else reuses existing, already-decided machinery. Requiring
`--name` (rather than inventing a default like `stdin` or deriving one from a
`/dev/fd` path) keeps the behaviour identical and predictable on every platform,
including extension-blind and `/dev`-less systems.

OPEN (ergonomics, not blocking 0.1): making `--name` OPTIONAL when the invoking
shell exposes the source through process substitution or `/dev/fd` (so a name
could be inferred) -- platform-specific and deferred; 0.1 always requires
`--name` with `-`.

### 5.4f Wildcard expansion of add/update FILE INPUTS -- DECIDED

PROBLEM (verified, cross-platform). On POSIX the shell expands an unquoted
`*.md` into a list of matching paths BEFORE walrus runs, so `walrus add base.wal
*.md` works. On Windows, cmd.exe and PowerShell do NOT expand `*.md`; walrus
receives the LITERAL operand `*.md`, tries to open a file named `*.md`, and
fails (on Windows, os error 123, "The filename, directory name, or volume label
syntax is incorrect"). The single most natural Windows invocation therefore
broke under the earlier design, which treated wildcard expansion of file INPUTS
as the shell's job and left the internal matcher (section 5.4c) for stored names
only.

DECISION: walrus itself expands a file-input operand that contains wildcard
metacharacters, using a PORTABLE glob (the `glob` crate), so `add *.md` and
`update *.md` behave IDENTICALLY on Windows and POSIX. On POSIX the shell may
still pre-expand first; walrus then sees literal paths and MUST NOT re-glob them
(the literal-path-wins rule below guarantees that).

SCOPE. This rule applies ONLY to the FILE-INPUT operands (the sources) of `add`
and `update` -- the files to ingest. It does NOT apply to:
- the `<archive>` path (always a literal path);
- a `-` stdin operand (section 5.4e);
- the `--name <stored-name>` value (a verbatim stored name, section 5.4e).
The stored-name mask matcher for `query` / `extract` / `remove` (section 5.4c)
is UNCHANGED by this rule; it is a different mechanism operating on stored names,
not filesystem paths.

PER-OPERAND RESOLUTION. Each file-input operand is resolved independently, in
this exact order:

1. LITERAL-PATH-WINS. If the operand matches an EXISTING literal path on disk
   (a file OR a directory), use it AS-IS -- no globbing. This is the POSIX /
   pre-expanded case (the shell already produced literal paths), and it also
   lets a real file whose name legitimately contains a glob metacharacter win
   over glob interpretation. Checking the literal path FIRST is what makes the
   feature a no-op on an already-expanded command line.
2. GLOB. ELSE if the operand contains a glob metacharacter (`*`, `?`, or `[`),
   expand it with the portable glob relative to the CURRENT WORKING DIRECTORY,
   collect the matches, and SORT them into a DETERMINISTIC order (lexicographic
   over the matched paths), then treat that sorted set as the operands in place
   of the pattern. If the pattern matches NOTHING, that is an ERROR: print a
   clear diagnostic to stderr (e.g. `walrus: no files match pattern '<p>'`) and
   exit RC 2. See "No-match return code" below for why RC 2.
3. LITERAL, NON-EXISTENT, NO METACHARACTERS. ELSE (the operand has no glob
   metacharacter and names no existing path) behaviour is UNCHANGED from today:
   the existing "file not found" read error (a general non-zero failure, as the
   current `std::fs::read` path produces).

NO-MATCH RETURN CODE: RC 2 (usage error). CHOSEN after inspecting the section 6
return-code table and `dispatch.rs`. RC 8 is reserved for "target file does not
exist IN THE ARCHIVE" (a stored-name miss on `update` / `remove` / `condense` /
`convert`); a glob that matches nothing on the FILESYSTEM at add/update time is
not an archive-membership miss -- it is an unsatisfiable operand supplied on the
command line, which is squarely a usage error. RC 2 is also what the adjacent
operand-validation case already uses (a directory operand without `-R`, sections
5.3 / 5.4b), so a malformed/unsatisfiable file operand and a no-match pattern
report the SAME class of error. This keeps the spec's own RC logic consistent.

INTERACTION WITH `-R`. Globbing happens FIRST (operand -> a set of resolved
paths); THEN each resolved path is handled by the existing add/update logic,
including the `-R` directory rule (section 5.4b). `-R` governs how a DIRECTORY
among the resolved paths is treated (descend vs. RC 2), UNCHANGED. A `*.md`
pattern that matches only FILES therefore behaves identically with or without
`-R`. The glob itself does NOT recurse implicitly: a plain `*` stays within one
directory level and does NOT cross `/` (so `*.md` matches `README.md` but not
`docs/guide.md`); only `**` crosses segments and recurses. This matches the
segment-aware `*` / `**` / `?` semantics the stored-name matcher already defines
(section 5.4c), so the two mechanisms read consistently even though they run on
different inputs.

PATH SEPARATORS AND DRIVE QUALIFICATION. On Windows the pattern accepts BOTH `/`
and `\` as separators. A bare pattern (`*.md`) expands relative to the current
working directory; a drive-qualified or absolute pattern
(`c:\workspace\Walrus\*.md`, or `/home/user/*.md`) expands against that location.
Both forms must work. Quote and escape handling MUST NOT mangle a Windows
drive-qualified pattern -- the `c:\...` prefix is a path, not an escape sequence.

BINARY / TRANSACTIONAL. The existing rules apply to the EXPANDED set exactly as
to a literal multi-file add. A binary file among the matches is rejected
(section 2.2.2) and, under the batch transaction (section 5.4a), aborts the
WHOLE add/update: no partial archive is written, the original is left
byte-for-byte unchanged. A no-change save among the matches is the success no-op
of section 5.4 (skipped with a stderr note, does not fail the batch).

DETERMINISM. Expansion sorts its matches (step 2) so a given working-directory
state always produces the SAME batch order and therefore the SAME resulting
archive, satisfying the deterministic-output rule in the testing convention --
the same guarantee `-R` recursion already provides (section 5.4b).

LIBRARY/FORMAT IMPACT: NONE. This is CLI argument handling (operand expansion
before any byte is read), not a change to the on-disk format or to any
`walrus-fmt` primitive -- the expanded paths funnel through the same
`Archive::add` / `Archive::update` calls a literal multi-file add already uses.
FORMAT.md is UNAFFECTED (INV-1): no record, manifest, or codec field changes.

OPEN (ergonomics, not blocking 0.1): a flag to opt OUT of walrus-side expansion
(for a user who wants a literal `*` operand on a system where the shell did not
expand and no file matches) -- not needed now, since literal-path-wins already
covers a real file named with a metacharacter, and a no-match pattern is a clear
RC 2 rather than a silent mis-open.

### 5.5 Remove (new)

```
walrus remove <archive> <file-mask> [--force]   # canonical
walrus <archive> <file-mask> -r [--force]         # alias
```

- Deletes the selected file(s) -- base and change log -- from the archive
  entirely.
- If no tracked file matches: fail RC 8 (consistent with update/condense).
- DECIDED (safety): if a wildcard mask matches MORE THAN ONE file, `--force` is
  required. Without it, remove refuses, lists what would be deleted, and exits
  RC 2 (the library's `AmbiguousRemove` error, section 6). A mask matching
  exactly one file does not need `--force`. A mask matching NO file is the
  `NotTracked` error -> RC 8.

### 5.6 Condense

```
walrus condense <archive> <file-mask> [--keep <n>]
```

- No short alias (condense is deliberately explicit, since it is destructive).
- Purpose: reduce bloat in archives whose change logs have grown large.
- Behavior: make the CURRENT version the new base and RE-INITIALISE the change
  log for the file. After condensing, the file has a fresh log (index 0 =
  current, no older indices); older history is discarded.
- DECIDED (retention): `--keep <n>` retains the N most recent versions and
  discards everything older. The default is `--keep 0` (collapse to current
  only). `--keep 5` would keep indices 0..=5 and drop the rest.
- `--keep <n>` is the one-shot manual form of the per-file generation cap
  (section 2.1) and shares the same re-base trimming logic. Condensing does not
  change the stored generation cap unless `--generations <n>` is also given.
- Accepts a single file, a list, or wildcard masks.
- If no tracked file matches: fail RC 8.

NOTE: condense is destructive -- it discards recoverable history. This is the
corrected definition; the earlier draft mistakenly described condense using
update's wording.

### 5.7 Convert (`.wal <-> .wbl`) -- DECIDED

```
walrus convert <archive> [file-mask] --to <wal|wbl> [-o <name>]
```

- Purpose: flip replay DIRECTION. `--to wal` makes the selected file(s) forward
  (original base + forward log); `--to wbl` makes them reverse (current base +
  reverse log). This changes the archive VARIANT/representation, not the content,
  which is why it lives in the CLI and is NOT an "export" (section 4 scope
  boundary).
- PER-FILE FLIP IS THE PRIMITIVE. With a `file-mask`, only matching files are
  converted; WITHOUT a mask, the WHOLE archive is converted (the no-mask case is
  the degenerate "all files" form of the same per-file operation).
- ROUND-TRIP GUARANTEE: convert inverts each splice and re-bases so that
  reconstruction is BYTE-IDENTICAL for every version index before and after; a
  forward -> reverse -> forward round-trip reproduces every index exactly
  (FR-W7 AC4, INV-3). `count` and checkpoints are preserved.
- MANIFEST `variant`: a WHOLE-ARCHIVE convert (no mask) MAY update the manifest
  `variant` to the new predominant direction and re-derives `base_role` /
  `log_direction` to match (so the manifest still validates on re-open). A MASKED
  convert leaves the archive `variant` untouched (the archive becomes mixed, which
  is legal -- direction is per file, section 6a).
- EXTENSION: convert MAY SUGGEST an extension rename (e.g. "converted to reverse;
  consider renaming to `*.wbl`") on stderr, but MUST NOT auto-rename the file on
  disk. The on-disk name changes only if the user passes an explicit `-o <name>`
  (write the converted archive under that name) or renames it themselves. This
  matches the add seed-order rule that the extension is a convenience, never
  authoritative over the manifest.
- Transactional like every other mutation (section 5.4a): a failed convert leaves
  the original archive byte-for-byte unchanged.
- No-match mask -> RC 8 (target does not exist), consistent with the other
  mask-driven commands.

---

## 6. Return codes

| Code | Meaning |
|------|---------|
| 0    | Success (includes a no-change add/update no-op, section 5.4) |
| 2    | Usage error (unknown option, mutually exclusive operations, bad args, an add/update file-input pattern matching no files per section 5.4f) |
| 4    | Extract index beyond the last recorded change; original returned with a warning |
| 8    | Target file does not exist in the archive (update / remove / condense / convert) |

DECIDED: RC 2 for usage errors (standard GNU convention), including the
multi-match remove-without-`--force` case.

TYPED ERROR -> RETURN CODE MAPPING. The `walrus-fmt` library surfaces typed
errors (FR-W7); the CLI maps them to return codes so the doc contract and the
library error enum stay aligned:

| Library error (`WalrusError`) | Trigger | Return code |
|-------------------------------|---------|-------------|
| `NotTracked` | `update` / `remove` / `condense` / `convert` on a file absent from the archive | 8 |
| `AmbiguousRemove` | `remove` with a mask matching more than one file and no `--force` | 2 |
| `BinaryInput` | a non-text (binary / invalid-UTF-8 / NUL-bearing) input to `add` / `update` | general failure, non-zero; aborts the batch transaction (5.4a) |
| `SpliceNotCharBoundary` | an editor-supplied splice that falls mid-UTF-8-character (library/editor path; not reachable from the file-oriented CLI) | general failure, non-zero |

A beyond-last-index selector on `extract` / `cat` is NOT an error of this class:
it writes the ORIGINAL version with a warning and exits RC 4 (sections 5.2.1 /
5.2.2).

---

## 6a. `.wchg` direction attribute

Each inner `.wchg` log carries a DIRECTION field in its own header so it is
self-describing when extracted standalone:

- A single enum/byte field: `forward` or `reverse`.
  - `forward` -- the log replays on top of the ORIGINAL base to move toward the
    current version (the `.wal` sense).
  - `reverse` -- the log replays on top of the CURRENT base to move toward the
    original version (the `.wbl` sense).
- A reader that opens a bare `.wchg` outside any archive uses this field to know
  which base the log expects and which direction to replay, without consulting a
  parent archive's extension.
- The exact byte layout (where the field sits in the `.wchg` header) is deferred
  to the FORMAT.md rewrite; the requirement here is only that the attribute
  EXISTS and is readable standalone.
- NO CLI SURFACE BY DESIGN: standalone `.wchg` replay (reconstructing a bare
  `.wchg` + its base outside any archive, FR-W9) is a `walrus-fmt` LIBRARY
  capability (consumed by Walker's history viewer, section 6b.6). No `walrus` or
  `walx` subcommand operates on a bare `.wchg`; the CLI always works on a whole
  `.wal`/`.wbl` archive. This omission is intentional, not a gap.

### Record timestamps are UTC -- DECIDED

Each change record carries an optional `ts` field (FORMAT.md 4.1). When present
it is a UTC ISO-8601 instant in the FIXED shape `YYYY-MM-DDThh:mm:ssZ` -- the
trailing `Z` (Zulu / UTC) is REQUIRED. Rationale and rules:

- STORE UTC, NEVER LOCAL TIME. UTC is unambiguous and totally ordered; local time
  is not (it repeats an hour across a DST fall-back and is meaningless once an
  archive moves between machines or timezones). The `--at <T>` snapshot selector
  (section 5.2.1) needs a monotonic order to resolve "the version as at T", which
  only UTC provides. The library's timestamp parser accepts ONLY this fixed `...Z`
  shape and rejects any offset or local-time string, so a non-UTC `ts` would
  silently fail timestamp addressing -- another reason the writer must emit UTC.
- `walrus add` / `update` STAMP EACH SAVE. When the CLI records a new version it
  sets that record's `ts` to the save instant formatted as UTC
  `YYYY-MM-DDThh:mm:ssZ` (seconds resolution), so `--at` works on CLI-produced
  archives. A record with no `ts` is simply skipped by `--at` matching (an
  archive whose writer did not stamp records supports `--index`/`--version` but
  not meaningful `--at`).
- DISPLAY IS SEPARATE. The stored and COMPARED value is always UTC; a UI (Walker,
  or a future `walrus log`) MAY render timestamps in the viewer's local time for
  humans, but never stores or addresses by local time. (Same convention as git:
  an absolute instant is stored; tools display local.)
- DETERMINISM NOTE: fixtures and tests must use FIXED timestamp strings, not the
  wall clock, per the testing steering -- only the live `add`/`update` path reads
  the real clock.

---

## 6b. Walker GUI: full walrus frontend

DECISION CHANGE: Walker is now intended to be a FULL GUI FRONTEND for the entire
walrus capability, not just a read/playback viewer. It exposes the same
operations as the `walrus` CLI (query / extract-as-export / add / update /
remove / condense / set-generations) through a graphical file navigator.

Dependency boundary (invariant 2) is PRESERVED: Walker calls the `walrus-fmt`
library's read AND write/mutate API -- the same API the `walrus` CLI uses. The
library stays GUI-free; both frontends sit on top of it. Making Walker a full
frontend is ADDITIVE (it needs the library's mutate surface), not a boundary
violation. GUI dependencies remain only in Walker.

### 6b.1 File navigator: two-pane explorer

- EXTERNAL pane: a real filesystem browser to select external file(s) for
  import.
- INTERNAL pane: the archive's tracked files, the GUI equivalent of
  `walrus query -l`, showing per file: name, attributes, index size, generation
  cap, and a bloat indicator (section 6b.3).
  - Supports selecting files singly OR in numbers (multi-select) for batch
    export, batch remove, and batch condense.
- The viewer/player operates on the SELECTED file's version timeline (its index
  range); the version picker for export is driven by that timeline.

### 6b.2 Import = add or update (resolved automatically)

DECIDED: the navigator offers ONE "Import" action, not separate Add/Update
buttons. It resolves add-vs-update for the user instead of exposing the CLI's
internal distinction:

- Imported name NOT present in the archive -> ADD (new tracked file). No prompt.
- Imported name ALREADY present -> default to UPDATE (new version of the existing
  file), but surface it: prompt "<name> exists -- import as a new version?" with
  the alternative to add under a different name.

Rationale: the user thinks "import this file"; the GUI hides the sharp edge the
CLI's add (create-or-update) vs update (refuse-if-missing) split exposes. The
confirmation appears only on a genuine name collision.

### 6b.3 Bloat indicator (color coding)

DECIDED: color-code each internal file row to flag logs worth condensing, driven
by an HONEST metric, not raw size.

- Metric: a BLOAT RATIO = `log_bytes / current_version_bytes` (how much history
  weighs relative to live content), optionally combined with index count. A long
  document with few edits is NOT bloated; a tiny file with thousands of churny
  edits IS -- raw size alone would mislead.
- Traffic-light scheme per row:
  - GREEN -- healthy; log small relative to content.
  - AMBER -- getting heavy; condense worth considering.
  - RED -- bloated; condense recommended.
- Advisory ONLY: never auto-condenses (condense is destructive). A tooltip shows
  the actual numbers (index count, log bytes, ratio).
- Interaction with the generation cap: a file with `generations = N` is already
  bounded, so the indicator is most meaningful for `generations = 0` (unlimited)
  files. Capped files reflect churn-within-window or stay green, so the two
  features do not visually contradict each other.

DECIDED (threshold defaults): the amber/red cutoffs are on the bloat ratio
`R = log_bytes / current_version_bytes`, with a secondary index-count floor so a
tiny-but-churny file still trips:

- GREEN:  `R < 2.0`  (history under 2x the live content).
- AMBER:  `2.0 <= R < 5.0`.
- RED:    `R >= 5.0`.
- Index-count floor: regardless of ratio, stay GREEN while index count `< 50`
  (a file with few versions is never "bloated", however small the base). Above
  50 versions the ratio bands apply.

These are STARTING defaults chosen for sane behavior without real-world data, not
tuned constants. They are user-configurable, and expected to be revised once
real archives exist. A capped file (`generations = N > 0`) is already bounded, so
its indicator reflects churn within the window and will typically stay green.

### 6b.4 Per-file and batch controls

- Per-file (context menu / details panel on the selected file): set/change
  generation cap, condense now (with `--keep N` exposed as a field), remove,
  export selected version.
- Batch (multi-select): export, remove (with the same multi-match confirmation
  the CLI's `--force` provides), condense.
- All mutations go through the library's transactional path (section 5.4a) --
  the GUI never writes a partial archive.

### 6b.5 Export is Walker-exclusive -- DECIDED

Content transformation to other presentation formats (txt/md -> html, docx, pdf,
json, csv, ...) lives ONLY in `walker`. The `walrus` CLI has no export (section
4, SCOPE BOUNDARY).

- Walker owns the transformer backends (the `export/` module) and their heavy
  dependencies; these are GUI-side (or behind `walrus-fmt` optional features that
  only `walker` enables), never pulled into the base CLI.
- The transformers operate on a file's RECONSTRUCTED bytes at the selected
  version, so they sit cleanly on top of the library's reconstruct output and do
  not touch the archive layout -- preserving the dependency boundary.
- Export in Walker: pick a tracked file + version in the navigator, choose a
  target format, write the transformed output. Per-file and batch (multi-select)
  export as listed in 6b.4.

### 6b.6 `.wchg` history viewer (Walker viewer) -- DECIDED

Walker gains a dedicated HISTORY / CHANGE-LOG viewer for a tracked file's `.wchg`
log, alongside the content viewers (text / code / markdown / html / csv / rich
text). Motivation: a `.wchg` is readable (UTF-8 JSONL: a header line then one
JSON splice per line) but NOT intuitive -- raw byte-offset splices
(`delete N at offset M, insert S`) are hard for a human to picture. The viewer
turns the raw log into something understandable.

Requirements:
- Render each change record as a human-readable BEFORE/AFTER (or inline)
  highlighted diff: inserted bytes highlighted (e.g. green), deleted bytes
  (e.g. red), positioned within the reconstructed version the record applied to.
- Show a TIMELINE of records (index, timestamp, optional note) that the user can
  step through, reusing the player/transport.
- Reads via the library's reconstruct output and the parsed `.wchg` records --
  no archive-layout coupling, boundary preserved.
- STANDALONE: because a `.wchg` is self-describing (header carries `direction`,
  section 6a), the viewer can open a bare `.wchg` extracted outside an archive,
  not only one inside an open archive.

EXPORTABLE AS A MARKDOWN CHANGE REPORT -- DECIDED: the history viewer can export
the change history as a Markdown REPORT of changes made -- a readable per-file
changelog (each version: index, timestamp, note, and the diff) suitable for
review or sharing. This is an export (Walker-side, section 6b.5), produced from
the `.wchg` records + reconstructions, so it stays within the dependency
boundary.

---

## 7. Open questions summary

DECIDED:
- Full management tool is named `walrus` (NOT `wal`). No realistic `$PATH`
  collision; same-name projects in other domains are a known non-blocking caveat.
- `walx` stays as the extract-only tool.
- Inner log extension is `.wchg`, one extension for both variants, with a
  `forward`/`reverse` direction attribute in its header (section 6a) so an
  extracted log is self-describing.
- External CLI follows POSIX / GNU parameter conventions.
- CLI shape is SUBCOMMANDS (`walrus <op> <archive> [mask]`); flag spellings kept
  as aliases.
- "Index size" in query = count of recorded versions; byte size of the log is a
  separate `-l` column.
- `extract` RESTORES file(s) to disk (archive-tool convention; default CWD,
  `-o <dir>`, refuse-on-existing unless `--force`); a separate `cat` writes ONE
  file's bytes to STDOUT for piping (section 5.2). `walx` provides both and
  nothing that mutates.
- EXPORT (content transformation txt/md -> html/docx/pdf/...) is WALKER-ONLY
  (sections 4 and 6b.5). The CLI never transforms content; it emits a file in its
  stored format only. `.wal <-> .wbl` convert is a variant change, not an export,
  and stays in the CLI. Transformer backends and their heavy deps stay out of the
  base CLI (invariant 2).
- DIFF ENGINE (section 2.2): use the `similar` crate (dependency-free, pure Rust)
  behind a thin diff-to-splice adapter in `walrus-fmt`; escalate to `imara-diff`
  (git-grade) only if large-file perf demands it. Do not hand-roll. Diff
  non-uniqueness does not threaten byte-identical reconstruction.
- DIFF GRANULARITY ADAPTS to content (section 2.2.1): line-oriented for files
  with line terminators, word/char for unbroken long text; output is ALWAYS
  byte-offset splices; granularity recorded as a `.wchg` header hint.
- BINARY FILES OUT OF SCOPE for now (section 2.2.2, Option A): add/update reject
  binary with a clear error; a binary-delta record variant is a documented
  future extension (extensible `op` enum).
- INTERNATIONAL / ENCODINGS (section 2.3): UTF-8 is the universal internal
  storage (covers Chinese/Japanese/Korean/Arabic/etc -- it is the international
  choice, not a limit). Non-UTF-8 source files (GBK/GB18030/Shift-JIS/Big5/
  windows-125x) are transcoded to UTF-8 on add via `encoding_rs` and the original
  encoding is recorded per file (restorable on extract). Character-granularity
  diffing is grapheme-aware so multi-byte scripts never split mid-character.
- `.wchg` HISTORY VIEWER (section 6b.6): a Walker viewer that renders the change
  log as a stepped before/after highlighted diff with a timeline; opens a bare
  `.wchg` standalone (self-describing header); EXPORTABLE as a Markdown change
  report.
- Remove requires `--force` when a wildcard matches more than one file (else
  RC 2).
- Condense takes `--keep <n>`, defaulting to `--keep 0` (collapse to current).
- Usage errors return RC 2.
- Each `.wchg` carries a GENERATIONS cap (section 2.1): N = rolling window of N
  versions, 0 = unlimited (default); enforced automatically on add/update by
  re-basing out the oldest. Settable via `--generations <n>`.
- Archive mutations (add/update/remove/condense) are TRANSACTIONAL via
  temp-file-plus-atomic-rename (section 5.4a) -- all-or-nothing, no corruption
  window.
- Walker becomes a FULL GUI FRONTEND for all walrus operations via a two-pane
  file navigator (section 6b): external-file import, internal-file multi-select
  export, plus add/update/remove/condense/set-generations -- all through the
  `walrus-fmt` library (boundary preserved).
- Import is ONE action resolving to add (new name) or update (existing name),
  prompting only on a name collision (section 6b.2).
- A per-row BLOAT INDICATOR (green/amber/red on a `log_bytes/current_bytes`
  ratio) advises when to condense; advisory only, never auto-condenses
  (section 6b.3).

- Bloat-indicator thresholds set to starting defaults (section 6b.3): green
  `R<2`, amber `2<=R<5`, red `R>=5` on `R = log_bytes/current_bytes`, with a
  50-version floor below which a file stays green. Configurable, to be retuned
  with real data.

STILL OPEN (implementation / follow-up, not design blockers):
1. (none at the design level -- the FORMAT.md rewrite below is the remaining
   authoritative-spec task; see FORMAT.md once updated.)
