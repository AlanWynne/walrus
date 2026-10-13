@echo off
rem ============================================================================
rem Walrus manual round-trip harness: back up a real source tree into a .wal
rem archive, extract it back out, and byte-compare the two trees.
rem
rem Round-trip is: copy real source -> walrus add -R -> walrus extract -> fc /B.
rem A clean compare ends with "trees are identical" (compareDir.bat exits 0).
rem
rem walrus roots stored names at the SOURCE dir's last path segment, so
rem `extract -o <dir>` reproduces the tree under <dir>\<source-basename>\.
rem Here the source is always %BASE% (segment "Base"), so every extracted tree
rem lands under <work>\Base and each compare targets that sub-path.
rem ============================================================================

rem -- 0. Paths --------------------------------------------------------------
set "REPO=C:\workspace\Walrus"
set "TEMPDIR=C:\workspace\Walrus-Temp-TestData"
set "BASE=C:\workspace\Walrus-Temp-TestData\Base"
set "WORK0000=C:\workspace\Walrus-Temp-TestData\Work0000"
set "WORK0001=C:\workspace\Walrus-Temp-TestData\Work0001"
set "WORK0002=C:\workspace\Walrus-Temp-TestData\Work0002"
set "WORK0003=C:\workspace\Walrus-Temp-TestData\Work0003"
set "WORK0004=C:\workspace\Walrus-Temp-TestData\Work0004"

set "WALRUS=C:\workspace\Walrus\target\debug\walrus.exe"
set "REPORT=%REPO%\.gate\tcr\WalrusTestHarness-Report.md"

rem -- 1. Cleanup before start -----------------------------------------------
rem      Wipe and recreate the temp area so no previous run's extracted tree
rem      survives to be re-ingested (the self-nesting Work0000\Work0000\... bug).

echo.
echo [STEP 1] Cleanup: wiping and recreating %TEMPDIR%
rmdir /S /Q "%TEMPDIR%" 2>nul
mkdir "%TEMPDIR%"
cd /d "%TEMPDIR%"

rem Initialise the report file (summary ledger, separate from full stdout).
rem The report records, per measurable step, the test description, expectation,
rem and PASS/FAIL (or MANUAL for visual-only checks). %PASS%/%FAIL% totals are
rem tallied by the :report subroutine and printed at the end.
set /a PASS=0
set /a FAIL=0
rem Markdown report. Table pipes are written as ^| so cmd does not treat them
rem as a command pipe. The results table header + separator are emitted here;
rem the :report / :reportnz / :manual subroutines append one table row each.
rem The report now lands in the repo's .gate\tcr\ dir (git-ignored), beside the
rem Python TCR, so both reports sit together; TEMPDIR holds scratch only.
if not exist "%REPO%\.gate\tcr" mkdir "%REPO%\.gate\tcr"
> "%REPORT%" echo # Walrus Test Harness Report
>> "%REPORT%" echo.
>> "%REPORT%" echo - **Generated:** %DATE% %TIME%
>> "%REPORT%" echo - **Binary:** `%WALRUS%`
>> "%REPORT%" echo.
>> "%REPORT%" echo ^| Step ^| Result ^| Test ^| Expectation ^|
>> "%REPORT%" echo ^| ---- ^| ------ ^| ---- ^| ----------- ^|

rem -- 2. Rebuild the source tree to back up ---------------------------------
rem      Copy only real source (*.md, *.rs) and EXCLUDE build output, nested
rem      worktrees, git metadata, the .gate logs, and this harness's own
rem      TestData area. Excluding TestData is what stops the harness from
rem      re-ingesting a previous run's output.

echo.
echo [STEP 2] Building source tree: robocopy *.md/*.rs from %REPO% to %BASE%
robocopy "%REPO%" "%BASE%" *.md *.rs /S /XD target .worktrees .git .gate node_modules TestData

rem -- 3. Create a brand new .wal archive from BASE --------------------------
rem      Point walrus at the BASE directory with -R to capture the WHOLE
rem      recursive tree (BASE has nested subdirs). A bare "%BASE%\*.md" would
rem      only match top-level .md files, so -R on the directory is what this
rem      whole-tree round-trip needs. (walrus now self-expands a wildcard file
rem      operand like *.md itself -- CLI-DESIGN 5.4f -- so that form works on
rem      Windows too; STEP 20 exercises it directly. It is just not what THIS
rem      whole-tree step wants.)

echo.
echo [STEP 3] Creating archive: walrus add %WORK0000%.wal %BASE% --wal -R
mkdir "%WORK0000%"
"%WALRUS%" add "%WORK0000%.wal" "%BASE%" --wal -R

rem -- 4. Extract the new archive back out -----------------------------------

echo.
echo [STEP 4] Extracting archive: walrus extract %WORK0000%.wal -o %WORK0000%
"%WALRUS%" extract "%WORK0000%.wal" -o "%WORK0000%"

rem -- 5. Byte-compare original BASE vs the extracted tree --------------------
rem      Extracted tree lives under %WORK0000%\Base (see header note).

echo.
echo [STEP 5] Round-trip check: compare BASE against the freshly extracted tree
echo " ============================================================================ "
echo " Comparing: "
echo " 		"%BASE%" with "%WORK0000%\Base"
echo " Expectation: No Difference"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%BASE%" "%WORK0000%\Base"
call :report %ERRORLEVEL% 0 "STEP 5" "Round-trip: BASE vs freshly extracted tree" "No Difference"

rem -- 6. Re-add the SAME source (%BASE%) a second time ----------------------
rem      Content is unchanged, so walrus should record no new version (no-change
rem      no-op). Adding the extracted %WORK0000% here instead would store a
rem      second tree under a different root segment and corrupt the idempotency
rem      check -- use %BASE%.

echo.
echo [STEP 6] No-op re-add: walrus add %WORK0000%.wal %BASE% --wal -R (expect no new version)
"%WALRUS%" add "%WORK0000%.wal" "%BASE%" --wal -R

rem -- 7. Copy the archive, re-add BASE to the copy, compare the two archives -
rem      After adding exactly the same content again, nothing should change, so
rem      the original and the copy should stay byte-identical.

rem Use copy /Y, NOT xcopy. xcopy to a destination that does not yet exist
rem prompts "F = file or D = directory?" and hangs forever when stdout is
rem redirected. copy always treats the target as a file and /Y overwrites
rem without asking.
echo.
echo [STEP 7] Determinism check: copy archive, re-add BASE to the copy, compare both .wal files
copy /Y "%WORK0000%.wal" "%WORK0001%.wal"
echo " ============================================================================ "
echo " After : copy %WORK0000%.wal to %WORK0001%.wal"
echo " Comparing: "
echo " 		%WORK0000%.wal with %WORK0001%.wal"
echo " Expectation: No Difference"

fc /b "%WORK0000%.wal" "%WORK0001%.wal"
call :report %ERRORLEVEL% 0 "STEP 7a" "Archive byte-identical after copy (pre re-add)" "No Difference"

"%WALRUS%" add "%WORK0001%.wal" "%BASE%" --wal -R

echo " ============================================================================ "
echo " After : %WALRUS% add %WORK0001%.wal %BASE% --wal -R"
echo " Comparing: "
echo " 		%WORK0000%.wal with %WORK0001%.wal"
echo " Expectation: No Difference"

fc /b "%WORK0000%.wal" "%WORK0001%.wal"
call :report %ERRORLEVEL% 0 "STEP 7b" "Archive byte-identical after no-op re-add (determinism)" "No Difference"

rem -- 8. Extract the copy and compare it to BASE ----------------------------
rem      Both archives still hold the pristine content, so the extract of the
rem      copy should match BASE exactly.

echo.
echo [STEP 8] Extract the copy and compare it to BASE
"%WALRUS%" extract "%WORK0001%.wal" -o "%WORK0001%"
echo " ============================================================================ "
echo " After : %WALRUS% extract %WORK0001%.wal -o %WORK0001%
echo " Comparing: "
echo " 		"%BASE%" with "%WORK0001%\Base"
echo " Expectation: No Difference"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%BASE%" "%WORK0001%\Base"
call :report %ERRORLEVEL% 0 "STEP 8" "Extract of the copy archive matches BASE" "No Difference"

rem -- 9. Mutate SOME files in the extracted WORK0000 tree -------------------
rem      Recurse into the extracted tree (files live under %WORK0000%\Base\...)
rem      and only rewrite files that actually contain OLD_TEXT, so SOME files
rem      change and some do not -- that is what proves changed files gain a new
rem      version while unchanged files stay put.

set "OLD_TEXT=tar"
set "NEW_TEXT=This old tar went to a bar"

echo.
echo [STEP 9] Mutating files under %WORK0000%\Base: replacing '%OLD_TEXT%' where present
c:\tools\powershell7\pwsh -NoProfile -Command ^
  "Get-ChildItem -LiteralPath '%WORK0000%\Base' -Recurse -File | ForEach-Object { $p=$_.FullName; $c=Get-Content -LiteralPath $p -Raw; if ($c -and $c.Contains('%OLD_TEXT%')) { $c.Replace('%OLD_TEXT%','%NEW_TEXT%') | Set-Content -LiteralPath $p -NoNewline } }"

rem -- 10. Confirm the mutated tree now differs from BASE --------------------

echo.
echo [STEP 10] Confirm the mutated tree now differs from BASE
echo " ============================================================================ "
echo " After : Changing '%OLD_TEXT%' -> '%NEW_TEXT%' in %WORK0000%\Base
echo " Comparing: "
echo " 		"%BASE%" with "%WORK0000%\Base"
echo " Expectation: Differences"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%BASE%" "%WORK0000%\Base"
call :report %ERRORLEVEL% 1 "STEP 10" "Mutated tree now differs from BASE" "Differences"

rem -- 11. Add the mutated tree to the copy archive, then extract it ---------
rem      The mutated tree was extracted rooted at "Base", so point the add at
rem      %WORK0000%\Base so the stored names match the originals (same "Base/..."
rem      root) and walrus records new versions on exactly the changed files.

echo.
echo [STEP 11] Add mutated tree as a new version, extract, compare current vs mutated
rem Extract into a FRESH dir (%WORK0001%-cur). %WORK0001% already holds step 8's
rem pristine extract; re-extracting the mutated current over a populated dir can
rem skip/refuse existing files and leave stale pristine content, which would
rem falsely fail the compare. A clean target isolates the current version.
"%WALRUS%" add "%WORK0001%.wal" "%WORK0000%\Base" --wal -R
"%WALRUS%" extract "%WORK0001%.wal" -o "%WORK0001%-cur"

echo " ============================================================================ "
echo " After : %WALRUS% add %WORK0001%.wal %WORK0000%\Base --wal -R"
echo "   and : %WALRUS% extract %WORK0001%.wal -o %WORK0001%-cur"
echo " Comparing: "
echo " 		"%WORK0000%\Base" with "%WORK0001%-cur\Base"
echo " Expectation: Same (both are the mutated tree)"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%WORK0000%\Base" "%WORK0001%-cur\Base"
call :report %ERRORLEVEL% 0 "STEP 11" "New version current == mutated tree" "No Difference"

rem -- 12. Extract the ORIGINAL version and compare it to BASE ---------------
rem      The copy archive now holds both the pristine base version and the
rem      mutated current version. Extracting --version original must restore the
rem      pristine pre-change tree, matching BASE.

echo.
echo [STEP 12] Version addressing: extract --version original, compare to pristine BASE
"%WALRUS%" extract "%WORK0001%.wal" -o "%WORK0002%" --version original
echo " ============================================================================ "
echo " After : %WALRUS% extract %WORK0001%.wal -o %WORK0002% --version original
echo " Comparing: "
echo " 		"%BASE%" with "%WORK0002%\Base"
echo " Expectation: Same (version original restores the pristine pre-change tree)"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%BASE%" "%WORK0002%\Base"
call :report %ERRORLEVEL% 0 "STEP 12" "Version 'original' restores pristine BASE" "No Difference"

rem -- 13. .wbl (reverse) variant round-trip ---------------------------------
rem      The two variants are symmetric: a .wbl archive (current base + reverse
rem      log) must round-trip the SAME source byte-for-byte as the .wal did.
rem      Build a fresh reverse archive from the pristine BASE, extract, compare.

echo.
echo [STEP 13] .wbl variant round-trip: walrus add --wbl, extract, compare to BASE
"%WALRUS%" add "%WORK0003%.wbl" "%BASE%" --wbl -R
"%WALRUS%" extract "%WORK0003%.wbl" -o "%WORK0003%"
echo " ============================================================================ "
echo " After : %WALRUS% add %WORK0003%.wbl %BASE% --wbl -R  (then extract -o %WORK0003%)"
echo " Comparing: "
echo " 		%BASE% with %WORK0003%\Base"
echo " Expectation: No Difference (.wbl round-trips identically to .wal)"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%BASE%" "%WORK0003%\Base"
call :report %ERRORLEVEL% 0 "STEP 13" ".wbl reverse-variant round-trip matches BASE" "No Difference"

rem -- 14. .wal -> .wbl conversion round-trip --------------------------------
rem      `convert --to wbl` flips the pristine .wal archive to a .wbl written to
rem      a new file (no in-place rename). Extracting the converted archive must
rem      still reproduce BASE: conversion preserves content, only the variant
rem      and log direction change (the inter-convertibility invariant).

echo.
echo [STEP 14] Conversion round-trip: walrus convert --to wbl, extract, compare to BASE
"%WALRUS%" convert "%WORK0000%.wal" --to wbl -o "%WORK0004%.wbl"
"%WALRUS%" extract "%WORK0004%.wbl" -o "%WORK0004%"
echo " ============================================================================ "
echo " After : %WALRUS% convert %WORK0000%.wal --to wbl -o %WORK0004%.wbl  (then extract)"
echo " Comparing: "
echo " 		%BASE% with %WORK0004%\Base"
echo " Expectation: No Difference (convert preserves content across variants)"

call "%REPO%\tests\acceptance\TestData\compareDir.bat" "%BASE%" "%WORK0004%\Base"
call :report %ERRORLEVEL% 0 "STEP 14" ".wal to .wbl conversion round-trip matches BASE" "No Difference"

rem -- 15. Query listing (terse + long) --------------------------------------
rem      Exercise both query renderings against the multi-version copy archive.
rem      Terse is ls-style (one relative name per line); -l is the aligned
rem      Versions | LastWriteTime | Bytes | Name table. Visual check only --
rem      no automated assertion, just confirm it runs and renders.

echo.
echo [STEP 15] Query listing: terse then long (-l) against %WORK0001%.wal
echo " ============================================================================ "
echo " --- walrus query %WORK0001%.wal  (terse, ls-style) ---"
"%WALRUS%" query "%WORK0001%.wal"
echo " --- walrus query %WORK0001%.wal -l  (long, aligned table) ---"
"%WALRUS%" query "%WORK0001%.wal" -l
call :manual "STEP 15" "Query terse + long render" "In stdout, confirm terse is one relative name per line, and -l shows an aligned 'Versions LastWriteTime Bytes Name' table with populated LastWriteTime and a nonzero Versions count on changed files."

rem -- 16. Single-file add stored under its basename -------------------------
rem      A plain file argument (not a directory) must be stored under just its
rem      basename, not its full path. Add one file, then query and confirm the
rem      listing names it by basename. Visual check.

echo.
echo [STEP 16] Single-file add: store a lone file under its basename
"%WALRUS%" add "%WORK0000%-single.wal" "%BASE%\README.md" --wal
echo " ============================================================================ "
echo " After : %WALRUS% add %WORK0000%-single.wal %BASE%\README.md --wal"
echo " Expectation: query lists the file as 'README.md' (basename only, no path)"
"%WALRUS%" query "%WORK0000%-single.wal"
rem Automated check: the terse query output must contain the literal basename
rem "README.md" (not a path). findstr /L = literal, /C: = exact substring;
rem rc 0 = found. (A full relative path would read "Base/README.md" or similar,
rem so a bare "README.md" line confirms basename-only storage.)
"%WALRUS%" query "%WORK0000%-single.wal" | findstr /L /C:"README.md" >nul
call :report %ERRORLEVEL% 0 "STEP 16" "Single-file add stored under basename only" "query shows literal 'README.md'"

rem -- 17. Error handling: directory without -R is a usage error (RC 2) ------
rem      Adding a directory without -R must be refused with return code 2 and
rem      must NOT create an archive. We echo the return code so the log shows it.

echo.
echo [STEP 17] Error check: add a directory WITHOUT -R (expect refusal, RC 2)
"%WALRUS%" add "%WORK0000%-nodir.wal" "%BASE%" --wal
set "RC17=%ERRORLEVEL%"
call :report %RC17% 2 "STEP 17" "Add a directory without -R is refused" "RC 2 (usage error)"
echo " After : %WALRUS% add ... %BASE% --wal   (no -R)"
echo " Return code: %RC17%   Expectation: 2 (usage error, no archive created)"

rem -- 18. Error handling: query a missing archive fails cleanly -------------
rem      Querying an archive that does not exist must fail with a clear message
rem      and a non-zero return code, not a crash.

echo.
echo [STEP 18] Error check: query a NON-EXISTENT archive (expect clean non-zero RC)
"%WALRUS%" query "%TEMPDIR%\does-not-exist.wal"
set "RC18=%ERRORLEVEL%"
rem Pass criterion is "non-zero" (clean failure). :reportnz checks RC is not 0.
call :reportnz %RC18% "STEP 18" "Query a missing archive fails cleanly" "non-zero RC, no crash"
echo " After : %WALRUS% query %TEMPDIR%\does-not-exist.wal"
echo " Return code: %RC18%   Expectation: non-zero (clean failure, no crash)"

rem -- 19. Run the Python acceptance suite + TCR (guarded) -------------------
rem      If python is on PATH, run the automated Python suite against freshly
rem      built fixtures and regenerate the Python TCR into the SAME .gate\tcr\
rem      dir as this report. If python is absent, record a MANUAL skipped row
rem      and continue -- a missing interpreter must not hard-fail the harness.
rem      Redirections live ONLY on the raw python command lines; NO cmd
rem      metacharacter appears in any :report / :manual description text.

echo.
echo [STEP 19] Python acceptance suite + TCR (skipped if python is absent)
where python >nul 2>&1
if errorlevel 1 goto :step19_skip

set "WALRUS_BIN=%WALRUS%"
python -m unittest discover -s "%REPO%\tests\acceptance\walrus_cli" > "%TEMPDIR%\py-suite.log" 2>&1
set "RCPY=%ERRORLEVEL%"
call :report %RCPY% 0 "STEP 19" "Python acceptance suite over fixtures" "rc 0"
python "%REPO%\tests\acceptance\walrus_cli\run_tcr.py" > "%TEMPDIR%\py-tcr.log" 2>&1
goto :step19_done

:step19_skip
call :manual "STEP 19" "Python acceptance suite" "python not found -- Python suite skipped"

:step19_done

rem -- 20. Wildcard self-expansion: add *.md (CLI-DESIGN 5.4f) ---------------
rem      walrus now expands a wildcard file operand itself, so `add base.wal
rem      *.md` works on Windows cmd (which does NOT pre-expand the glob) exactly
rem      as it does on a POSIX shell. Build a small flat dir of three .md files,
rem      add with the LITERAL pattern, and confirm all three were stored.

echo.
echo [STEP 20] Wildcard self-expand: add *.md stores every matching file
set "GLOBDIR=%TEMPDIR%\globsrc"
mkdir "%GLOBDIR%"
echo one> "%GLOBDIR%\a.md"
echo two> "%GLOBDIR%\b.md"
echo three> "%GLOBDIR%\c.md"
echo notmd> "%GLOBDIR%\d.txt"
cd /d "%GLOBDIR%"
"%WALRUS%" add "%TEMPDIR%\glob.wal" *.md --wal
set "RC20=%ERRORLEVEL%"
cd /d "%TEMPDIR%"
rem Expect exactly the three .md files stored (d.txt excluded by the pattern).
"%WALRUS%" query "%TEMPDIR%\glob.wal" > "%TEMPDIR%\glob-query.log" 2>&1
rem Count stored names via find /c /v "" on the query output.
for /f %%N in ('type "%TEMPDIR%\glob-query.log" ^| find /c /v ""') do set "GLOBCOUNT=%%N"
echo " After : %WALRUS% add glob.wal *.md --wal   (literal pattern, cmd does not pre-expand)"
echo " add rc=%RC20%   stored names=%GLOBCOUNT%   Expectation: rc 0, 3 names"
call :report %RC20% 0 "STEP 20a" "add with literal *.md pattern succeeds" "rc 0"
call :report %GLOBCOUNT% 3 "STEP 20b" "wildcard *.md expands to exactly the 3 md files" "3 stored names"

rem -- 90. Report summary ----------------------------------------------------
echo.
echo ============================================================
echo  Test harness complete.  PASS=%PASS%  FAIL=%FAIL%
echo  Full report: %REPORT%
echo ============================================================
>> "%REPORT%" echo.
>> "%REPORT%" echo ## Totals
>> "%REPORT%" echo.
>> "%REPORT%" echo - **PASS:** %PASS%
>> "%REPORT%" echo - **FAIL:** %FAIL%
if %FAIL% GTR 0 goto :sum_fail
>> "%REPORT%" echo - **Overall:** PASS
goto :sum_done
:sum_fail
>> "%REPORT%" echo - **Overall:** FAIL
:sum_done
type "%REPORT%"

goto :END

rem ==========================================================================
rem Subroutines
rem ==========================================================================

rem :report <actual-rc> <expected-rc> <step> <description> <expectation>
rem   Compares actual vs expected return code, records PASS/FAIL to the report
rem   and the console, and tallies %PASS% / %FAIL%.
rem   NOTE: no literal parentheses in the echoed text, and goto-based branching
rem   rather than if(...)else(...), so cmd never mis-parses a stray "(".
rem   Each report line is a Markdown table row; table pipes are escaped as ^|.
:report
if "%~1"=="%~2" goto :report_pass
set /a FAIL+=1
echo [%~3] FAIL - %~4  [expected: %~5; got rc=%~1]
>> "%REPORT%" echo ^| %~3 ^| **FAIL** ^| %~4 ^| %~5 -- got rc=%~1 ^|
goto :EOF
:report_pass
set /a PASS+=1
echo [%~3] PASS - %~4  [expected: %~5]
>> "%REPORT%" echo ^| %~3 ^| **PASS** ^| %~4 ^| %~5 ^|
goto :EOF

rem :reportnz <actual-rc> <step> <description> <expectation>
rem   Passes when the actual return code is NON-ZERO (a clean, reported failure).
:reportnz
if "%~1"=="0" goto :reportnz_fail
set /a PASS+=1
echo [%~2] PASS - %~3  [expected: %~4; got rc=%~1]
>> "%REPORT%" echo ^| %~2 ^| **PASS** ^| %~3 ^| %~4 -- got rc=%~1 ^|
goto :EOF
:reportnz_fail
set /a FAIL+=1
echo [%~2] FAIL - %~3  [expected: %~4; got rc=0]
>> "%REPORT%" echo ^| %~2 ^| **FAIL** ^| %~3 ^| %~4 -- got rc=0 ^|
goto :EOF

rem :manual <step> <description> <what-to-verify>
rem   Records a visual-only check that has no automated pass/fail. The run
rem   cannot decide it; a human must read stdout and confirm the stated point.
:manual
echo [%~1] MANUAL - %~2
>> "%REPORT%" echo ^| %~1 ^| _MANUAL_ ^| %~2 ^| VERIFY: %~3 ^|
goto :EOF

:END

rem -- 99. (optional) cleanup ------------------------------------------------
rem rmdir /S /Q "%TEMPDIR%" 2>nul
