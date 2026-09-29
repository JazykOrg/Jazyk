#!/usr/bin/env bash
# crit_readme_3_433d6fb7
# req:readme-25
# Each chapter file begins with the chapter title as a level-one heading and
# contains only the chapter's prose after it, with no notes, front matter, or
# scene labels.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md … 08-the-lantern-room.md).
#
# This test checks each chapter file for:
#  1. First non-empty line is a level-one heading (# Title)
#  2. No YAML front matter (--- delimiters at the top)
#  3. No scene labels (lines matching common scene-label patterns)
#  4. No author notes (lines matching common note patterns)
set -u

fail=0

# Collect chapter files
chapters=()
for f in [0-9][0-9]-*.md; do
  if [ -f "$f" ]; then
    chapters+=("$f")
  fi
done

if [ "${#chapters[@]}" -eq 0 ]; then
  echo "FAIL: no chapter files found"
  exit 1
fi

for f in "${chapters[@]}"; do
  # 1. First non-empty line must be a level-one heading
  first_line=$(grep -m1 -v '^[[:space:]]*$' "$f" 2>/dev/null || true)
  if [ -z "$first_line" ]; then
    echo "FAIL: $f is empty"
    fail=1
    continue
  fi
  if ! echo "$first_line" | grep -qE '^# [^#]'; then
    echo "FAIL: $f does not begin with a level-one heading (got: $first_line)"
    fail=1
    continue
  fi

  # 2. No YAML front matter: no line that is exactly '---' before the heading
  #    (front matter would appear before or at the very top)
  if head -1 "$f" | grep -qE '^---[[:space:]]*$'; then
    echo "FAIL: $f has YAML front matter"
    fail=1
    continue
  fi

  # 3. No scene labels: lines like "Scene 1", "SCENE 1", "## Scene", etc.
  if grep -qiE '^[[:space:]]*(##?\s*)?scene[[:space:]]*[0-9]' "$f"; then
    echo "FAIL: $f contains a scene label"
    fail=1
    continue
  fi

  # 4. No author notes: lines like "NOTE:", "Author's note", "[Note:"
  if grep -qiE '^[[:space:]]*(NOTE:|Author.s note|\[Note:)' "$f"; then
    echo "FAIL: $f contains an author note"
    fail=1
    continue
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "FAIL: one or more chapter files violate the structure requirement"
  exit 1
fi

echo "PASS: all chapter files begin with a level-one heading and contain only prose"
exit 0
