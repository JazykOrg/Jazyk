#!/usr/bin/env bash
# crit_readme_21_9a9868c8
# req:readme-3
# Port Alder is written as two words.
#
# Criterion crit:readme-21 [positive]: A reader seeing the town name in the
# manuscript sees `Port Alder` as two separate words.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md ... 08-the-lantern-room.md).
#
# This is an orthography constraint: the town name must never appear joined as
# "PortAlder" (one word). The test scans every chapter file for the joined form
# and fails if it is found. With no chapter files yet there is nothing to verify,
# so the test fails by design until generation produces the manuscript.
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
  echo "FAIL: no chapter files found; the town name cannot be verified"
  exit 1
fi

for f in "${chapters[@]}"; do
  # The joined form "PortAlder" (case-insensitive, no space) violates the
  # two-word constraint.
  if grep -qiE 'PortAlder' "$f"; then
    echo "FAIL: $f writes the town name as one word (PortAlder)"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "FAIL: the town name is not consistently written as two words"
  exit 1
fi

echo "PASS: the town name is written as two words (Port Alder) throughout"
exit 0
