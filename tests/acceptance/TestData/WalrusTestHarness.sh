#!/usr/bin/env bash
# ============================================================================
# Walrus manual round-trip harness (bash port of WalrusTestHarness.bat).
# Back up a real source tree into a .wal archive, extract it back out, and
# byte-compare the two trees, plus variant/conversion/version/error checks.
#
# Round-trip is: copy real source -> walrus add -R -> walrus extract -> cmp.
# A clean compare ends with "trees are identical" (compareDir.sh exits 0).
#
# walrus roots stored names at the SOURCE dir's last path segment, so
# `extract -o <dir>` reproduces the tree under <dir>/<source-basename>/.
# Here the source is always $BASE (segment "Base"), so every extracted tree
# lands under <work>/Base and each compare targets that sub-path.
#
# A Markdown PASS/FAIL report is written to $REPORT. The console shows the
# full step-by-step chatter; the report is the clean summary ledger.
# ============================================================================
set -u

# -- 0. Paths --------------------------------------------------------------
# REPO defaults to the repository root inferred from this script's location
# (tests/acceptance/TestData -> three levels up), but can be overridden.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-$(cd "$SCRIPT_DIR/../../.." && pwd)}"
# Scratch work area (wiped and recreated each run). Defaults to a dedicated
# subfolder under C:\workspace\temp (/c/workspace/temp under Git Bash / MSYS)
# so the step-1 wipe only ever removes this harness's own scratch. Override
# TEMPDIR to relocate it (e.g. TMPDIR-based on a non-Windows host).
TEMPDIR="${TEMPDIR:-/c/workspace/temp/Walrus-TestData}"
BASE="$TEMPDIR/Base"
WORK0000="$TEMPDIR/Work0000"
WORK0001="$TEMPDIR/Work0001"
WORK0002="$TEMPDIR/Work0002"
WORK0003="$TEMPDIR/Work0003"
WORK0004="$TEMPDIR/Work0004"

# WALRUS defaults to the debug build; override to test a release binary.
WALRUS="${WALRUS:-$REPO/target/debug/walrus}"
REPORT="$REPO/.gate/tcr/WalrusTestHarness-Report.md"
COMPARE="$SCRIPT_DIR/compareDir.sh"

PASS=0
FAIL=0

# === report helpers =======================================================
# report <actual-rc> <expected-rc> <step> <description> <expectation>
report() {
    local actual="$1" expected="$2" step="$3" desc="$4" exp="$5"
    if [ "$actual" = "$expected" ]; then
        PASS=$((PASS + 1))
        echo "[$step] PASS - $desc  [expected: $exp]"
        printf '| %s | **PASS** | %s | %s |\n' "$step" "$desc" "$exp" >> "$REPORT"
    else
        FAIL=$((FAIL + 1))
        echo "[$step] FAIL - $desc  [expected: $exp; got rc=$actual]"
        printf '| %s | **FAIL** | %s | %s -- got rc=%s |\n' "$step" "$desc" "$exp" "$actual" >> "$REPORT"
    fi
}

# reportnz <actual-rc> <step> <description> <expectation>
# Passes when the actual return code is NON-ZERO (a clean, reported failure).
reportnz() {
    local actual="$1" step="$2" desc="$3" exp="$4"
    if [ "$actual" != "0" ]; then
        PASS=$((PASS + 1))
        echo "[$step] PASS - $desc  [expected: $exp; got rc=$actual]"
        printf '| %s | **PASS** | %s | %s -- got rc=%s |\n' "$step" "$desc" "$exp" "$actual" >> "$REPORT"
    else
        FAIL=$((FAIL + 1))
        echo "[$step] FAIL - $desc  [expected: $exp; got rc=0]"
        printf '| %s | **FAIL** | %s | %s -- got rc=0 |\n' "$step" "$desc" "$exp" >> "$REPORT"
    fi
}

# manual <step> <description> <what-to-verify>
manual() {
    echo "[$1] MANUAL - $2"
    printf '| %s | _MANUAL_ | %s | VERIFY: %s |\n' "$1" "$2" "$3" >> "$REPORT"
}

# === steps =================================================================

# -- 1. Cleanup before start -----------------------------------------------
echo
echo "[STEP 1] Cleanup: wiping and recreating $TEMPDIR"
rm -rf "$TEMPDIR"
mkdir -p "$TEMPDIR"
cd "$TEMPDIR"

# Initialise the Markdown report (summary ledger, separate from full stdout).
# The report now lands in the repo's .gate/tcr/ dir (git-ignored), beside the
# Python TCR, so both reports sit together; TEMPDIR holds scratch only.
mkdir -p "$REPO/.gate/tcr"
{
    echo "# Walrus Test Harness Report"
    echo
    echo "- **Generated:** $(date '+%Y-%m-%d %H:%M:%S')"
    echo "- **Binary:** \`$WALRUS\`"
    echo
    echo "| Step | Result | Test | Expectation |"
    echo "| ---- | ------ | ---- | ----------- |"
} > "$REPORT"

# -- 2. Rebuild the source tree to back up ---------------------------------
# Copy only real source (*.md, *.rs) and EXCLUDE build output, nested
# worktrees, git metadata, .gate logs, node_modules, and this harness's own
# TestData area (so the harness never re-ingests a previous run's output).
echo
echo "[STEP 2] Building source tree: copy *.md/*.rs from $REPO to $BASE"
mkdir -p "$BASE"
( cd "$REPO" && find . \
    \( -path './target' -o -path './.worktrees' -o -path './.git' \
       -o -path './.gate' -o -path './node_modules' \
       -o -path './tests/acceptance/TestData' \) -prune -o \
    -type f \( -name '*.md' -o -name '*.rs' \) -print0 \
  | while IFS= read -r -d '' f; do
        mkdir -p "$BASE/$(dirname "$f")"
        cp "$f" "$BASE/$f"
    done )

# -- 3. Create a brand new .wal archive from BASE --------------------------
echo
echo "[STEP 3] Creating archive: walrus add $WORK0000.wal $BASE --wal -R"
mkdir -p "$WORK0000"
"$WALRUS" add "$WORK0000.wal" "$BASE" --wal -R

# -- 4. Extract the new archive back out -----------------------------------
echo
echo "[STEP 4] Extracting archive: walrus extract $WORK0000.wal -o $WORK0000"
"$WALRUS" extract "$WORK0000.wal" -o "$WORK0000"

# -- 5. Byte-compare original BASE vs the extracted tree -------------------
echo
echo "[STEP 5] Round-trip check: compare BASE against the freshly extracted tree"
"$COMPARE" "$BASE" "$WORK0000/Base"
report $? 0 "STEP 5" "Round-trip: BASE vs freshly extracted tree" "No Difference"

# -- 6. Re-add the SAME source (BASE) a second time (no-op) ----------------
echo
echo "[STEP 6] No-op re-add: walrus add $WORK0000.wal $BASE --wal -R (expect no new version)"
"$WALRUS" add "$WORK0000.wal" "$BASE" --wal -R

# -- 7. Copy the archive, re-add BASE to the copy, compare the two archives -
echo
echo "[STEP 7] Determinism check: copy archive, re-add BASE to the copy, compare both .wal files"
cp -f "$WORK0000.wal" "$WORK0001.wal"
cmp -s "$WORK0000.wal" "$WORK0001.wal"
report $? 0 "STEP 7a" "Archive byte-identical after copy (pre re-add)" "No Difference"

"$WALRUS" add "$WORK0001.wal" "$BASE" --wal -R
cmp -s "$WORK0000.wal" "$WORK0001.wal"
report $? 0 "STEP 7b" "Archive byte-identical after no-op re-add (determinism)" "No Difference"

# -- 8. Extract the copy and compare it to BASE ----------------------------
echo
echo "[STEP 8] Extract the copy and compare it to BASE"
"$WALRUS" extract "$WORK0001.wal" -o "$WORK0001"
"$COMPARE" "$BASE" "$WORK0001/Base"
report $? 0 "STEP 8" "Extract of the copy archive matches BASE" "No Difference"

# -- 9. Mutate SOME files in the extracted WORK0000 tree -------------------
# Only rewrite files that contain OLD_TEXT, so some change and some do not.
OLD_TEXT="tar"
NEW_TEXT="This old tar went to a bar"
echo
echo "[STEP 9] Mutating files under $WORK0000/Base: replacing '$OLD_TEXT' where present"
while IFS= read -r -d '' f; do
    if grep -q "$OLD_TEXT" "$f" 2>/dev/null; then
        # Use a temp file so partial writes never corrupt the source on error.
        sed "s/$OLD_TEXT/$NEW_TEXT/g" "$f" > "$f.tmp" && mv "$f.tmp" "$f"
    fi
done < <(find "$WORK0000/Base" -type f -print0)

# -- 10. Confirm the mutated tree now differs from BASE --------------------
echo
echo "[STEP 10] Confirm the mutated tree now differs from BASE"
"$COMPARE" "$BASE" "$WORK0000/Base"
report $? 1 "STEP 10" "Mutated tree now differs from BASE" "Differences"

# -- 11. Add the mutated tree as a new version, extract, compare -----------
# Extract into a FRESH dir ($WORK0001-cur): $WORK0001 already holds step 8's
# pristine extract; re-extracting over a populated dir can leave stale files.
echo
echo "[STEP 11] Add mutated tree as a new version, extract, compare current vs mutated"
"$WALRUS" add "$WORK0001.wal" "$WORK0000/Base" --wal -R
"$WALRUS" extract "$WORK0001.wal" -o "$WORK0001-cur"
"$COMPARE" "$WORK0000/Base" "$WORK0001-cur/Base"
report $? 0 "STEP 11" "New version current == mutated tree" "No Difference"

# -- 12. Extract the ORIGINAL version and compare it to BASE ---------------
echo
echo "[STEP 12] Version addressing: extract --version original, compare to pristine BASE"
"$WALRUS" extract "$WORK0001.wal" -o "$WORK0002" --version original
"$COMPARE" "$BASE" "$WORK0002/Base"
report $? 0 "STEP 12" "Version 'original' restores pristine BASE" "No Difference"

# -- 13. .wbl (reverse) variant round-trip ---------------------------------
echo
echo "[STEP 13] .wbl variant round-trip: walrus add --wbl, extract, compare to BASE"
"$WALRUS" add "$WORK0003.wbl" "$BASE" --wbl -R
"$WALRUS" extract "$WORK0003.wbl" -o "$WORK0003"
"$COMPARE" "$BASE" "$WORK0003/Base"
report $? 0 "STEP 13" ".wbl reverse-variant round-trip matches BASE" "No Difference"

# -- 14. .wal to .wbl conversion round-trip --------------------------------
echo
echo "[STEP 14] Conversion round-trip: walrus convert --to wbl, extract, compare to BASE"
"$WALRUS" convert "$WORK0000.wal" --to wbl -o "$WORK0004.wbl"
"$WALRUS" extract "$WORK0004.wbl" -o "$WORK0004"
"$COMPARE" "$BASE" "$WORK0004/Base"
report $? 0 "STEP 14" ".wal to .wbl conversion round-trip matches BASE" "No Difference"

# -- 15. Query listing (terse + long) -- visual check ----------------------
echo
echo "[STEP 15] Query listing: terse then long (-l) against $WORK0001.wal"
echo " --- walrus query $WORK0001.wal  (terse, ls-style) ---"
"$WALRUS" query "$WORK0001.wal"
echo " --- walrus query $WORK0001.wal -l  (long, aligned table) ---"
"$WALRUS" query "$WORK0001.wal" -l
manual "STEP 15" "Query terse + long render" "In stdout, confirm terse is one relative name per line, and -l shows an aligned 'Versions LastWriteTime Bytes Name' table with populated LastWriteTime and a nonzero Versions count on changed files."

# -- 16. Single-file add stored under its basename -------------------------
echo
echo "[STEP 16] Single-file add: store a lone file under its basename"
"$WALRUS" add "$WORK0000-single.wal" "$BASE/README.md" --wal
"$WALRUS" query "$WORK0000-single.wal"
# A full relative path would read "Base/README.md"; a bare "README.md" line
# confirms basename-only storage. grep -Fx = fixed-string, whole-line match.
if "$WALRUS" query "$WORK0000-single.wal" | grep -Fxq "README.md"; then
    report 0 0 "STEP 16" "Single-file add stored under basename only" "query shows literal 'README.md'"
else
    report 1 0 "STEP 16" "Single-file add stored under basename only" "query shows literal 'README.md'"
fi

# -- 17. Error handling: directory without -R is a usage error (RC 2) ------
echo
echo "[STEP 17] Error check: add a directory WITHOUT -R (expect refusal, RC 2)"
"$WALRUS" add "$WORK0000-nodir.wal" "$BASE" --wal
RC17=$?
report "$RC17" 2 "STEP 17" "Add a directory without -R is refused" "RC 2 (usage error)"

# -- 18. Error handling: query a missing archive fails cleanly -------------
echo
echo "[STEP 18] Error check: query a NON-EXISTENT archive (expect clean non-zero RC)"
"$WALRUS" query "$TEMPDIR/does-not-exist.wal"
RC18=$?
reportnz "$RC18" "STEP 18" "Query a missing archive fails cleanly" "non-zero RC, no crash"

# -- 19. Run the Python acceptance suite + TCR (guarded) -------------------
# If python is available, run the automated Python suite against fresh fixtures
# and regenerate the Python TCR into the SAME .gate/tcr/ dir as this report. If
# python is absent, record a MANUAL skipped row and continue -- a missing
# interpreter must not hard-fail the harness. Redirections live ONLY on the raw
# python command lines; no metacharacter appears in any report description text.
echo
echo "[STEP 19] Python acceptance suite + TCR (skipped if python is absent)"
PYBIN="$(command -v python3 || command -v python || true)"
if [ -z "$PYBIN" ]; then
    manual "STEP 19" "Python acceptance suite" "python not found -- Python suite skipped"
else
    export WALRUS_BIN="$WALRUS"
    "$PYBIN" -m unittest discover -s "$REPO/tests/acceptance/walrus_cli" > "$TEMPDIR/py-suite.log" 2>&1
    RCPY=$?
    report "$RCPY" 0 "STEP 19" "Python acceptance suite over fixtures" "rc 0"
    "$PYBIN" "$REPO/tests/acceptance/walrus_cli/run_tcr.py" > "$TEMPDIR/py-tcr.log" 2>&1 || true
fi

# -- 20. Wildcard self-expansion: add *.md (CLI-DESIGN 5.4f) ---------------
# walrus expands a wildcard file operand itself, so `add glob.wal *.md` works
# even when the shell did NOT pre-expand it. Here we QUOTE the pattern so bash
# does not expand it, passing the literal "*.md" to walrus -- a faithful test
# of walrus's own expansion (and the Windows-cmd case). Build a small flat dir
# of three .md files plus a .txt, add with the literal pattern, confirm exactly
# the three .md files were stored.
echo
echo "[STEP 20] Wildcard self-expand: add *.md stores every matching file"
GLOBDIR="$TEMPDIR/globsrc"
mkdir -p "$GLOBDIR"
printf 'one\n'   > "$GLOBDIR/a.md"
printf 'two\n'   > "$GLOBDIR/b.md"
printf 'three\n' > "$GLOBDIR/c.md"
printf 'notmd\n' > "$GLOBDIR/d.txt"
( cd "$GLOBDIR" && "$WALRUS" add "$TEMPDIR/glob.wal" '*.md' --wal )
RC20=$?
GLOBCOUNT=$("$WALRUS" query "$TEMPDIR/glob.wal" | grep -c .)
echo " After : $WALRUS add glob.wal '*.md' --wal   (literal pattern, shell did not pre-expand)"
echo " add rc=$RC20   stored names=$GLOBCOUNT   Expectation: rc 0, 3 names"
report "$RC20" 0 "STEP 20a" "add with literal *.md pattern succeeds" "rc 0"
report "$GLOBCOUNT" 3 "STEP 20b" "wildcard *.md expands to exactly the 3 md files" "3 stored names"

# -- 21. Search: find a known string / no-match / case-insensitive ---------
# search is READ-ONLY: reconstruct each selected file at the chosen version and
# report where a string occurs. Exercised against the pristine $WORK0000.wal
# (the real .md/.rs source tree), which is certain to contain the literal
# "walrus". RC table (CLI-DESIGN 5.6): 0 found, 1 no match, 2 usage. The return
# code alone is the pass criterion.
echo
echo "[STEP 21] Search: a known literal is found - RC 0"
"$WALRUS" search "$WORK0000.wal" walrus > "$TEMPDIR/search-found.log" 2>&1
RC21A=$?
echo " After : $WALRUS search $WORK0000.wal walrus   (expect at least one hit)"
echo " Return code: $RC21A   Expectation: 0 (found)"
report "$RC21A" 0 "STEP 21a" "search finds a known literal string" "RC 0 (found)"

# The absent-needle is assembled from two halves at runtime so the literal
# token never appears in any tracked .md/.rs file (including this harness or the
# resume notes) -- otherwise STEP 2's copy would ingest it into the archive and
# the "no match" search would wrongly find it. Concatenating keeps the whole
# string out of the source tree.
NEEDLE_A="qzx7"
NEEDLE_B="k9vmwp"
ABSENT="${NEEDLE_A}${NEEDLE_B}"
echo "[STEP 21] Search: a string that is absent - RC 1"
"$WALRUS" search "$WORK0000.wal" "$ABSENT" > "$TEMPDIR/search-none.log" 2>&1
RC21B=$?
echo " After : $WALRUS search $WORK0000.wal <runtime-assembled absent token>   (expect no hits)"
echo " Return code: $RC21B   Expectation: 1 (no match)"
report "$RC21B" 1 "STEP 21b" "search of an absent string reports no matches" "RC 1 (no match)"

echo "[STEP 21] Search: case-insensitive -i finds a mixed-case needle - RC 0"
"$WALRUS" search "$WORK0000.wal" WALRUS -i > "$TEMPDIR/search-ci.log" 2>&1
RC21C=$?
echo " After : $WALRUS search $WORK0000.wal WALRUS -i   (case-folded hit)"
echo " Return code: $RC21C   Expectation: 0 (found via -i)"
report "$RC21C" 0 "STEP 21c" "case-insensitive search matches a mixed-case needle" "RC 0 (found)"

# -- 22. All-versions search over a known two-version archive ----------------
# Use a dedicated scratch archive so the searched token is guaranteed to exist
# in both the current and original versions.
ALLVERSIONSSRC="$TEMPDIR/allversions-source"
ALLVERSIONSARCHIVE="$TEMPDIR/allversions.wal"
mkdir -p "$ALLVERSIONSSRC"
printf 'search-version-probe\n' > "$ALLVERSIONSSRC/probe.txt"
"$WALRUS" add "$ALLVERSIONSARCHIVE" "$ALLVERSIONSSRC/probe.txt" --wal > "$TEMPDIR/allversions-add.log" 2>&1
RC22A=$?
report "$RC22A" 0 "STEP 22a" "create all-versions search fixture" "RC 0"
printf 'search-version-probe revised\n' > "$ALLVERSIONSSRC/probe.txt"
"$WALRUS" update "$ALLVERSIONSARCHIVE" "$ALLVERSIONSSRC/probe.txt" > "$TEMPDIR/allversions-update.log" 2>&1
RC22B=$?
report "$RC22B" 0 "STEP 22b" "add a second fixture version" "RC 0"

echo
echo "[STEP 22] Search every version: expect matches at index 0 and index 1"
"$WALRUS" search "$ALLVERSIONSARCHIVE" search-version-probe --all-versions > "$TEMPDIR/allversions-search.log" 2>&1
RC22C=$?
report "$RC22C" 0 "STEP 22c" "all-versions search finds history matches" "RC 0"
grep -F -q "Versions: all" "$TEMPDIR/allversions-search.log"
RC22D=$?
report "$RC22D" 0 "STEP 22d" "all-versions report identifies its scope" "Versions all"
grep -F -q "Version index: 0" "$TEMPDIR/allversions-search.log"
RC22E=$?
report "$RC22E" 0 "STEP 22e" "all-versions report includes current version" "index 0"
grep -F -q "Version index: 1" "$TEMPDIR/allversions-search.log"
RC22F=$?
report "$RC22F" 0 "STEP 22f" "all-versions report includes original version" "index 1"

"$WALRUS" search "$ALLVERSIONSARCHIVE" search-version-probe --all-versions --index 0 > "$TEMPDIR/allversions-selector.log" 2>&1
RC22G=$?
report "$RC22G" 2 "STEP 22g" "all-versions rejects a version selector" "RC 2"

"$WALRUS" convert "$ALLVERSIONSARCHIVE" --to wbl -o "$TEMPDIR/allversions.wbl" > "$TEMPDIR/allversions-convert.log" 2>&1
RC22H=$?
report "$RC22H" 0 "STEP 22h" "convert the fixture to reverse-log form" "RC 0"
"$WALRUS" search "$TEMPDIR/allversions.wbl" search-version-probe --all-versions > "$TEMPDIR/allversions-wbl-search.log" 2>&1
RC22I=$?
report "$RC22I" 0 "STEP 22i" "all-versions search works on a reverse-log archive" "RC 0"
grep -F -q "Version index: 0" "$TEMPDIR/allversions-wbl-search.log"
RC22J=$?
report "$RC22J" 0 "STEP 22j" "reverse-log report includes current version" "index 0"
grep -F -q "Version index: 1" "$TEMPDIR/allversions-wbl-search.log"
RC22K=$?
report "$RC22K" 0 "STEP 22k" "reverse-log report includes original version" "index 1"

# -- 90. Report summary ----------------------------------------------------
echo
echo "============================================================"
echo " Test harness complete.  PASS=$PASS  FAIL=$FAIL"
echo " Full report: $REPORT"
echo "============================================================"
{
    echo
    echo "## Totals"
    echo
    echo "- **PASS:** $PASS"
    echo "- **FAIL:** $FAIL"
    if [ "$FAIL" -gt 0 ]; then
        echo "- **Overall:** FAIL"
    else
        echo "- **Overall:** PASS"
    fi
} >> "$REPORT"
cat "$REPORT"

# -- 99. (optional) cleanup ------------------------------------------------
# rm -rf "$TEMPDIR"

# Exit non-zero if any automated check failed (MANUAL steps do not count).
[ "$FAIL" -eq 0 ]
