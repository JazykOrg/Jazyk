#!/usr/bin/env bash
# crit_style_11
# req:style-11
# Each chapter runs between 2,000 and 3,500 words.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md ... 08-the-lantern-room.md).
#
# This requirement is a word-count rule with four boundary criteria that pin the
# edges of the allowed interval [2000, 3500]:
#   crit_style_3  a chapter with 2,000 words satisfies the rule (lower edge, inclusive)
#   crit_style_4  a chapter with 3,500 words satisfies the rule (upper edge, inclusive)
#   crit_style_5  a chapter with 1,999 words is outside the allowed length
#   crit_style_6  a chapter with 3,501 words is outside the allowed length
#
# The test drives the artifact: it inspects the actual manuscript chapter files
# and checks that every chapter's word count is within the closed interval
# [2000, 3500]. The boundary criteria pin the edges of that interval.
#
# When no chapter files exist yet, the manuscript is not generated and the test
# fails by design: it is the acceptance gate generation must clear.
#
# Usage: bash tests/crit_style_11.sh <test-name>
# Each entry's run selects its own test by name.
set -u

MIN=2000
MAX=3500

# Collect the actual manuscript chapter files, in reading order.
chapters=()
for f in [0-9][0-9]-*.md; do
  if [ -f "$f" ]; then
    chapters+=("$f")
  fi
done

name="${1:-}"

# No chapter files: the manuscript is not yet generated. Fail by design.
if [ "${#chapters[@]}" -eq 0 ]; then
  echo "FAIL: no manuscript chapter files found; the manuscript is not yet generated"
  exit 1
fi

# Every chapter must be within the allowed length [2000, 3500].
out_of_range=0
for f in "${chapters[@]}"; do
  n=$(wc -w < "$f" | tr -d ' ')
  if [ "$n" -lt "$MIN" ] || [ "$n" -gt "$MAX" ]; then
    echo "FAIL: $f has $n words, outside the allowed 2,000-3,500 range"
    out_of_range=1
  fi
done

if [ "$out_of_range" -ne 0 ]; then
  echo "FAIL: one or more chapters fall outside the allowed chapter length"
  exit 1
fi

case "$name" in
  crit_style_3_10cc2c85)
      # Lower edge, inclusive: a chapter at 2,000 words is in range.
    echo "PASS: every chapter is within the allowed length; the 2,000-word lower edge is inclusive"
    exit 0
      ;;
  crit_style_4_e7f4e120)
      # Upper edge, inclusive: a chapter at 3,500 words is in range.
    echo "PASS: every chapter is within the allowed length; the 3,500-word upper edge is inclusive"
    exit 0
      ;;
  crit_style_5_8ba54e39)
      # Just below the lower edge: a chapter at 1,999 words is out of range.
    echo "PASS: no chapter falls below the 2,000-word lower edge"
    exit 0
      ;;
  crit_style_6_4e1f10c4)
      # Just above the upper edge: a chapter at 3,501 words is out of range.
    echo "PASS: no chapter exceeds the 3,500-word upper edge"
    exit 0
      ;;
  *)
    echo "FAIL: unknown test name '$name'"
    exit 2
      ;;
esac
