#!/usr/bin/env bash
# Byte-compare two directory trees (bash port of compareDir.bat).
#   Usage: compareDir.sh "<DIR1>" "<DIR2>"
#   DIR1 = first (reference) tree, e.g. the original source backed up.
#   DIR2 = second tree to compare against it, e.g. the extracted copy.
# Reports files MISSING FROM DIR2, DIFFERENT (byte mismatch), or EXTRA IN DIR2.
# Exit code: 0 = identical, 1 = differences found, 2 = usage/missing-dir error.
set -u

DIR1="${1:-}"
DIR2="${2:-}"

if [ -z "$DIR1" ] || [ -z "$DIR2" ]; then
    echo "Usage: compareDir.sh \"<DIR1>\" \"<DIR2>\""
    echo "  Compares two directory trees byte-for-byte."
    exit 2
fi
if [ ! -d "$DIR1" ]; then
    echo "ERROR: DIR1 does not exist: $DIR1"
    exit 2
fi
if [ ! -d "$DIR2" ]; then
    echo "ERROR: DIR2 does not exist: $DIR2"
    exit 2
fi

diffcount=0

echo "Comparing directories..."
echo "  DIR1: $DIR1"
echo "  DIR2: $DIR2"
echo

# Files present under DIR1: report MISSING (absent in DIR2) or DIFFERENT.
while IFS= read -r -d '' f; do
    rel="${f#"$DIR1"/}"
    if [ ! -e "$DIR2/$rel" ]; then
        echo "MISSING FROM DIR2: $rel"
        diffcount=$((diffcount + 1))
    elif ! cmp -s "$DIR1/$rel" "$DIR2/$rel"; then
        echo "DIFFERENT: $rel"
        diffcount=$((diffcount + 1))
    fi
done < <(find "$DIR1" -type f -print0)

# Files present under DIR2 but absent in DIR1: report EXTRA.
while IFS= read -r -d '' f; do
    rel="${f#"$DIR2"/}"
    if [ ! -e "$DIR1/$rel" ]; then
        echo "EXTRA IN DIR2: $rel"
        diffcount=$((diffcount + 1))
    fi
done < <(find "$DIR2" -type f -print0)

echo
if [ "$diffcount" -eq 0 ]; then
    echo "Comparison complete: trees are identical."
    exit 0
else
    echo "Comparison complete: $diffcount difference(s) found."
    exit 1
fi
