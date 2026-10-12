@echo off
rem Byte-compare two directory trees.
rem   Usage: compareDir.bat "<DIR1>" "<DIR2>"
rem   DIR1 = first (reference) tree, e.g. the original source backed up.
rem   DIR2 = second tree to compare against it, e.g. the extracted copy.
rem Reports files MISSING FROM DIR2, DIFFERENT (byte mismatch), or EXTRA IN DIR2.
rem Quote any path containing spaces; surrounding quotes are stripped on read.

set "DIR1=%~1"
set "DIR2=%~2"

if "%DIR1%"=="" goto :USAGE
if "%DIR2%"=="" goto :USAGE
if not exist "%DIR1%\" (
    echo ERROR: DIR1 does not exist: %DIR1%
    exit /b 2
)
if not exist "%DIR2%\" (
    echo ERROR: DIR2 does not exist: %DIR2%
    exit /b 2
)

set "DIFFCOUNT=0"

echo Comparing directories...
echo   DIR1: %DIR1%
echo   DIR2: %DIR2%
echo.

for /R "%DIR1%" %%F in (*) do (
    call :COMPARE "%%F"
)

for /R "%DIR2%" %%F in (*) do (
    call :CHECKEXTRA "%%F"
)

echo.
if "%DIFFCOUNT%"=="0" (
    echo Comparison complete: trees are identical.
    exit /b 0
) else (
    echo Comparison complete: %DIFFCOUNT% difference^(s^) found.
    exit /b 1
)

:USAGE
echo Usage: compareDir.bat "^<DIR1^>" "^<DIR2^>"
echo   Compares two directory trees byte-for-byte.
exit /b 2

:COMPARE
set "FILE=%~1"
call set "REL=%%FILE:%DIR1%\=%%"

if not exist "%DIR2%\%REL%" (
    echo MISSING FROM DIR2: %REL%
    set /a DIFFCOUNT+=1
    goto :EOF
)

fc /B "%DIR1%\%REL%" "%DIR2%\%REL%" >nul
if errorlevel 1 (
    echo DIFFERENT: %REL%
    set /a DIFFCOUNT+=1
)
goto :EOF

:CHECKEXTRA
set "FILE=%~1"
call set "REL=%%FILE:%DIR2%\=%%"

if not exist "%DIR1%\%REL%" (
    echo EXTRA IN DIR2: %REL%
    set /a DIFFCOUNT+=1
)
goto :EOF