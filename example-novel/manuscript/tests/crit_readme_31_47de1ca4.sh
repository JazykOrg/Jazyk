#!/usr/bin/env bash
# crit_readme_31_47de1ca4
# req:readme-5
# Theo's surname is Brandt, spelled with a t.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md ... 08-the-lantern-room.md).
#
# This test checks the manuscript prose for two things:
#    1. The surname "Brandt" (with a final t) appears in the manuscript.
#    2. No occurrence of the surname "Brand" without the final t appears
#       (a "Brand" not immediately followed by "t" would be the surname
#       wrongly spelled).
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

# 1. The surname "Brandt" must appear somewhere.
if ! grep -qE 'Brandt' "${chapters[@]}" 2>/dev/null; then
  echo "FAIL: the surname 'Brandt' does not appear in the manuscript"
  fail=1
fi

# 2. No "Brand" that is not immediately followed by "t" (the surname without t).
for f in "${chapters[@]}"; do
  while IFS= read -r line; do
    # A "Brand" not followed by "t" is the surname wrongly spelled.
    if printf '%s' "$line" | grep -qE 'Brand([^t]|$)'; then
      echo "FAIL: $f has a 'Brand' not spelled 'Brandt' (surname missing the final t): $line"
      fail=1
    fi
  done < <(grep -nE 'Brand' "$f" 2>/dev/null || true)
done

if [ "$fail" -ne 0 ]; then
  echo "FAIL: Theo's surname is not consistently spelled 'Brandt' with a final t"
  exit 1
fi

echo "PASS: Theo's surname appears as 'Brandt' with a final t"
exit 0
