"""The authoritative requirement registry for the walrus CLI acceptance suite.

Every id below is a coverage contract: the TCR generator (``run_tcr.py``)
reports a COVERAGE GAP (and a FAIL sign-off verdict) for any id that has no
covering ``@covers(...)`` test. Keep this list in exact sync with the plan's
"Registry ids" section; the generator reads only this module.

The ids are grouped by their source document (FR-W10 / FR-W11 acceptance
criteria from requirements-walrus.md, plus the CLI-DESIGN command sections).
"""

from __future__ import annotations


# A stable id -> one-line human description. The description is what the TCR
# prints beside each id in the traceability matrix.
REGISTRY: dict[str, str] = {
    # --- FR-W10 (walx extract/cat ACs; the walrus CLI honours the same ones) --
    "FR-W10-AC1": "extract restores selected files under stored names; existing "
    "target refused unless --force",
    "FR-W10-AC2": "extract numeric index beyond last -> original + warning + RC 4",
    "FR-W10-AC3": "cat requires exactly one match (RC 2 else); raw bytes to "
    "stdout; never writes disk",
    "FR-W10-AC4": "selectors current/original/index/relative/at; two selectors "
    "RC 2; malformed --at RC 2",
    # --- FR-W11 (walrus CLI ACs) ------------------------------------------
    "FR-W11-AC1": "query lists files; -l aligned Versions/LastWriteTime/Bytes/Name table",
    "FR-W11-AC2": "two operations -> RC 2",
    "FR-W11-AC3": "update/remove/condense on a non-existent target -> RC 8",
    "FR-W11-AC4": "convert flips per-file direction with --to, round-trips, no "
    "auto-rename on disk",
    "FR-W11-AC5": "add --wal/--wbl/--direction override only at create; ignored "
    "for an already-tracked file",
    "FR-W11-AC6": "subcommand behaviour verified over fixtures",
    "FR-W11-AC7": "-R / --recurse on add and update",
    "FR-W11-AC7a": "directory without -R -> RC 2",
    "FR-W11-AC7b": "-R stores <dir>/<relative> and extract reproduces the tree",
    "FR-W11-AC7c": "deterministic sorted walk; regular files only",
    "FR-W11-AC7d": "binary file in the walk aborts the batch; archive unchanged",
    "FR-W11-AC7e": "-R update of an untracked recursed file -> RC 8, unchanged",
    "FR-W11-AC8": "add/update stamp = source-file mtime; wall-clock fallback for stdin",
    "FR-W11-AC9": "add/update read stdin when the operand is '-', with --name",
    "FR-W11-AC9a": "cmd | add - --name foo creates/updates; cat reproduces bytes",
    "FR-W11-AC9b": "'-' without --name -> RC 2; --name without '-' -> RC 2",
    "FR-W11-AC9c": "'-' mixed with a file / '-' twice / -R with '-' -> RC 2",
    "FR-W11-AC9d": "non-UTF-8 / NUL stdin rejected; archive unchanged",
    "FR-W11-AC9e": "update - --name on an untracked name -> RC 8",
    "FR-W11-AC9f": "--name stored verbatim; escaping/illegal --name -> RC 2",
    # --- CLI-DESIGN command sections --------------------------------------
    "CLI-DESIGN-2.1-shortid-longpath": "deep long-path add -R round-trips, no "
    "ustar overflow",
    "CLI-DESIGN-5.1-query-terse": "terse ls-style: one bare name per line, sorted",
    "CLI-DESIGN-5.1-query-long": "-l aligned table: Versions / LastWriteTime / Bytes / Name",
    "CLI-DESIGN-5.1-query-lastwrite": "query -l LastWriteTime is the source-file mtime, "
    "latest version on a multi-version file",
    "CLI-DESIGN-5.1-query-mask": "segment-aware mask filter (* vs **)",
    "CLI-DESIGN-5.2.1-extract-output": "-o recreates nested relative tree",
    "CLI-DESIGN-5.2.1-extract-overwrite": "refuse RC 2 unless --force",
    "CLI-DESIGN-5.2.2-cat-stdout": "exact bytes to stdout, no disk write",
    "CLI-DESIGN-5.3-add-create": "create under .wal and .wbl",
    "CLI-DESIGN-5.3-add-basename": "single-file arg stored under basename",
    "CLI-DESIGN-5.3-add-scale": "many-file mixed-depth tree add -R round-trips "
    "byte-identical at scale",
    "CLI-DESIGN-5.3-add-noop-deterministic": "a no-op re-add of unchanged "
    "content leaves the archive byte-identical",
    "CLI-DESIGN-5.3-direction": "--wal/--wbl/--direction seed + conflict RC 2",
    "CLI-DESIGN-5.3-generations": "--generations caps + trims",
    "CLI-DESIGN-5.4-update-refuse": "update of an untracked file -> RC 8",
    "CLI-DESIGN-5.4-nochange": "byte-identical save -> RC 0 + note, no new version",
    "CLI-DESIGN-5.4a-transactional": "failed batch leaves archive unchanged",
    "CLI-DESIGN-5.4b-recurse": "directory recursion stored-name rules",
    "CLI-DESIGN-5.4c-mask-segment": "/-canonical names; segment-aware matching",
    "CLI-DESIGN-5.4d-sanitize": "--sanitize is accepted and never relaxes the "
    "path-escape refusal (the remap+report branch needs an archive with an "
    "unsafe stored name, which the CLI refuses to CREATE, so that branch is "
    "driven by the Rust safety.rs unit tests, not this black-box suite)",
    "CLI-DESIGN-5.4d-refuse": "default refuses unrepresentable name RC 2",
    "CLI-DESIGN-5.4d-zipslip": "path-escape refused RC 2 even with sanitize/force",
    "CLI-DESIGN-5.4e-stdin": "'-' + --name stdin ingestion rules",
    "CLI-DESIGN-5.4f-add-glob": "add expands a wildcard file operand itself "
    "(portable glob), identical on Windows and POSIX",
    "CLI-DESIGN-5.4f-update-glob": "update expands a wildcard file operand "
    "itself, exactly as add",
    "CLI-DESIGN-5.4f-literal-wins": "an existing literal path (incl. a "
    "shell-pre-expanded name) is used as-is, never re-globbed",
    "CLI-DESIGN-5.4f-nomatch-rc2": "a wildcard matching no file is RC 2 with a "
    "clear diagnostic",
    "CLI-DESIGN-5.4f-no-recurse": "a plain * does not cross / (no implicit "
    "recursion); ** recurses",
    "CLI-DESIGN-5.5-remove-single": "single match removes without force",
    "CLI-DESIGN-5.5-remove-multi": "multi-match needs --force (RC 2 else)",
    "CLI-DESIGN-5.5-remove-nomatch": "no match -> RC 8",
    "CLI-DESIGN-5.6-condense-keep0": "collapse to current only",
    "CLI-DESIGN-5.6-condense-keepn": "retain n versions",
    "CLI-DESIGN-5.6-condense-nomatch": "no match -> RC 8",
    "CLI-DESIGN-5.7-convert-roundtrip": "whole-archive flip byte-identical per index",
    "CLI-DESIGN-5.7-convert-masked": "masked flip only matched; mixed archive legal",
    "CLI-DESIGN-5.7-convert-norename": "no auto-rename; -o writes a new file",
    "CLI-DESIGN-5.7-convert-nomatch": "no match -> RC 8",
    "CLI-DESIGN-4-alias-install": "install into isolated --dir creates our shim",
    "CLI-DESIGN-4-alias-status": "status reports ours",
    "CLI-DESIGN-4-alias-remove": "remove deletes only ours",
    "CLI-DESIGN-4-alias-foreign": "foreign wal -> install RC 2; --force still refuses",
    "CLI-DESIGN-4-alias-idempotent": "re-install over our shim idempotent",
    "CLI-DESIGN-4-help": "--help/-h/help -> RC 0 + usage",
    "CLI-DESIGN-4-unknown-option": "unknown option -> RC 2",
    "CLI-DESIGN-6-rc0": "at least one RC 0 case",
    "CLI-DESIGN-6-rc2": "at least one RC 2 case",
    "CLI-DESIGN-6-rc4": "at least one RC 4 case",
    "CLI-DESIGN-6-rc8": "at least one RC 8 case",
    "CLI-DESIGN-1.4-codec-zstd": "written archive begins with zstd magic",
    "CLI-DESIGN-1.4-codec-gzip-defer": "gzip-magic file errors cleanly (deferral)",
    "CLI-DESIGN-1.4-codec-garbage": "random-magic file errors cleanly, no panic",
    "CLI-DESIGN-6a-utc-stamp": "--at works on a CLI-produced archive (AC8 proof)",
    "CLI-DESIGN-5-deep-history": "deep single-file history (~50 mixed-size "
    "versions) restores byte-exact at every index via --index/--version/--at",
    # --- search (CLI-DESIGN 5.8 search; shared engine, walrus + walx) -----
    "CLI-DESIGN-5.8-search-found": "a literal match prints a Markdown report "
    "(archive/index/timestamp header + Line/Col/Byte-offset/Match table) and RC 0",
    "CLI-DESIGN-5.8-search-nomatch": "a valid zero-hit search -> RC 1 (grep "
    "convention; search-specific no-match code)",
    "CLI-DESIGN-5.8-search-ignore-case": "-i/--ignore-case matches mixed case; "
    "case-sensitive search of the same needle finds nothing",
    "CLI-DESIGN-5.8-search-plain": "--format plain omits the Markdown table and "
    "lists file:line hits",
    "CLI-DESIGN-5.8-search-mask": "a file-mask restricts the search to matching "
    "tracked files",
    "CLI-DESIGN-5.8-search-selector": "a version selector searches that version; "
    "two selectors -> RC 2",
    "CLI-DESIGN-5.8-search-beyond-last": "a numeric index beyond the last version "
    "clamps and exits RC 4",
    "CLI-DESIGN-5.8-search-regex": "--regex matches a pattern (search-regex "
    "feature enabled in the CLI binaries)",
    "CLI-DESIGN-5.8-search-all-versions": "--all-versions searches current "
    "through original and reports matching version indices",
    "CLI-DESIGN-5.8-search-all-versions-selector-conflict": "--all-versions "
    "combined with a version selector is RC 2",
    "CLI-DESIGN-6-rc1": "at least one RC 1 case (search no-match)",
}


def all_ids() -> set[str]:
    """The full set of requirement ids that must be covered."""
    return set(REGISTRY.keys())


# The registry keys ARE the id set; this guards against an accidental dup key
# (a dict literal would silently drop one, which must never pass unnoticed).
assert len(REGISTRY) == len(all_ids()), "duplicate id in REGISTRY"


if __name__ == "__main__":
    print(f"requirements registry: {len(REGISTRY)} ids")
