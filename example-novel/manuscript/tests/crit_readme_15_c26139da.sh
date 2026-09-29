#!/usr/bin/env bash
# crit_readme_15_c26139da
# req:readme-23
# The chapter files are numbered in reading order and named after the chapter
# title as manuscript/01-arrival.md ... manuscript/08-the-lantern-room.md.
#
# The deliverable directory IS the manuscript root, so the chapter files live
# at the top level. This test asserts all eight exist in reading order.
set -u

expected=(
  "01-arrival.md"
  "02-the-survey.md"
  "03-the-ledger.md"
  "04-the-quay.md"
  "05-the-council.md"
  "06-the-storm.md"
  "07-the-notebook.md"
  "08-the-lantern-room.md"
)

missing=0
for f in "${expected[@]}"; do
  if [ ! -f "$f" ]; then
    echo "MISSING: $f"
    missing=1
  fi
done

if [ "$missing" -ne 0 ]; then
  echo "FAIL: not all chapter files present in reading order"
  exit 1
fi

echo "PASS: all eight chapter files present in reading order"
exit 0
