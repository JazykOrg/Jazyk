#!/usr/bin/env bash
# crit_readme_16_028032fd
# req:readme-24
# The manuscript has eight chapters.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md … 08-the-lantern-room.md).
# This test counts chapter files and asserts exactly eight.
set -u

count=0
for f in [0-9][0-9]-*.md; do
  if [ -f "$f" ]; then
    count=$((count + 1))
  fi
done

if [ "$count" -ne 8 ]; then
  echo "FAIL: expected 8 chapter files, found $count"
  exit 1
fi

echo "PASS: manuscript has exactly eight chapters"
exit 0
