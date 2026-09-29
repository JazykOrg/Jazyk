#!/usr/bin/env bash
# crit_readme_11_f480c734
# req:readme-4
# The Lantern Room is capitalized as a proper name, while the lantern in its
# window is not capitalized.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md ... 08-the-lantern-room.md).
#
# This test checks the manuscript prose for two things:
#   1. The shop is referred to by its capitalized proper name "Lantern Room"
#      (i.e. "The Lantern Room" / "the Lantern Room").
#   2. The object in the window is referred to by the lowercase "lantern";
#      no capitalized "Lantern" appears that is NOT part of the proper name
#      "Lantern Room" (a capitalized standalone "Lantern" would be the object
#      wrongly capitalized).
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
  echo "FAIL: no chapter files found; the manuscript is not yet generated"
  exit 1
fi

# 1. The proper name "Lantern Room" must appear capitalized somewhere.
if ! grep -qE 'Lantern Room' "${chapters[@]}" 2>/dev/null; then
  echo "FAIL: the proper name 'Lantern Room' does not appear capitalized in the manuscript"
  fail=1
fi

# 2. No capitalized "Lantern" that is not part of "Lantern Room".
#    A capitalized "Lantern" not immediately followed by " Room" is the object
#    wrongly capitalized.
for f in "${chapters[@]}"; do
  while IFS= read -r line; do
    # Strip out the proper-name occurrences "Lantern Room".
    stripped=$(printf '%s' "$line" | sed 's/Lantern Room//g')
    if printf '%s' "$stripped" | grep -qE 'Lantern'; then
      echo "FAIL: $f has a capitalized 'Lantern' that is not the proper name 'Lantern Room' (object wrongly capitalized): $line"
      fail=1
    fi
  done < <(grep -nE 'Lantern' "$f" 2>/dev/null || true)
done

if [ "$fail" -ne 0 ]; then
  echo "FAIL: capitalization of 'The Lantern Room' vs the lowercase 'lantern' is not consistent"
  exit 1
fi

echo "PASS: 'The Lantern Room' is capitalized as a proper name and the window object is lowercase 'lantern'"
exit 0
