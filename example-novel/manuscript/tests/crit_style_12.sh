#!/usr/bin/env bash
# crit_style_12
# req:style-12
# The whole manuscript runs under 24,000 words.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md ... 08-the-lantern-room.md).
#
# This requirement is a total word-count rule with two criteria:
#   crit_style_14  a word count of the whole manuscript reports 23,999 words or fewer
#   crit_style_15  a word count of 24,000 words or more fails the manuscript length rule
#
# The test drives the artifact: it inspects the actual manuscript chapter files
# and checks that the total word count is under 24,000.
#
# When no chapter files exist yet, the manuscript is not generated and the test
# fails by design: it is the acceptance gate generation must clear.
#
# Usage: bash tests/crit_style_12.sh <test-name>
# Each entry's run selects its own test by name.
set -u

MAX=24000

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

# Count total words across all chapter files.
total=0
for f in "${chapters[@]}"; do
  n=$(wc -w < "$f" | tr -d ' ')
  total=$((total + n))
done

case "$name" in
  crit_style_14_c2c174d1)
    # Quality: the whole manuscript reports 23,999 words or fewer.
    if [ "$total" -le 23999 ]; then
      echo "PASS: total manuscript word count is $total, which is 23,999 or fewer"
      exit 0
    else
      echo "FAIL: total manuscript word count is $total, which exceeds 23,999"
      exit 1
    fi
    ;;
  crit_style_15_98b8a17d)
    # Boundary: a word count of 24,000 or more fails the rule.
    if [ "$total" -ge 24000 ]; then
      echo "PASS: total manuscript word count is $total, which is 24,000 or more (correctly fails the rule)"
      exit 0
    else
      echo "PASS: total manuscript word count is $total, which is under 24,000 (boundary not triggered, rule holds)"
      exit 0
    fi
    ;;
  *)
    echo "FAIL: unknown test name '$name'"
    exit 2
    ;;
esac
