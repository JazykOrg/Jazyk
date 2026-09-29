#!/usr/bin/env bash
# crit_readme_5_9114c25c
# req:readme-6
# Chapter titles in the manuscript match the chapter titles in the plot.
#
# The deliverable directory IS the manuscript root. Chapter files are named
# NN-title.md (e.g. 01-arrival.md ... 08-the-lantern-room.md). The plot
# (../docs/plot.md) lists the chapter titles as "## Chapter N: Title" and
# states that the chapter titles are the file names of the deliverable without
# the number.
#
# This test checks that, in reading order, each manuscript chapter file's title
# (the part after "NN-") matches the corresponding plot chapter title, after
# normalizing case and spaces to hyphens.
set -u

PLOT="../docs/plot.md"

# 1. Expected titles from the plot: "## Chapter N: Title"
expected=()
if [ -f "$PLOT" ]; then
  while IFS= read -r line; do
    case "$line" in
      "## Chapter "*)
        rest="${line### Chapter }"
        title="${rest#*: }"
        norm=$(printf '%s' "$title" | tr 'A-Z' 'a-z' | sed 's/ /-/g; s/-\{2,\}/-/g; s/^-//; s/-$//')
        expected+=("$norm")
        ;;
    esac
  done < "$PLOT"
fi

# 2. Actual titles from the manuscript chapter files NN-title.md, in reading order
actual=()
for f in [0-9][0-9]-*.md; do
  [ -f "$f" ] || continue
  base="${f%.md}"
  title="${base#*-}"
  norm=$(printf '%s' "$title" | tr 'A-Z' 'a-z' | sed 's/ /-/g; s/-\{2,\}/-/g; s/^-//; s/-$//')
  actual+=("$norm")
done

if [ "${#actual[@]}" -eq 0 ]; then
  echo "FAIL: no manuscript chapter files found; the manuscript is not yet generated"
  exit 1
fi

if [ "${#expected[@]}" -ne "${#actual[@]}" ]; then
  echo "FAIL: chapter count mismatch: plot has ${#expected[@]}, manuscript has ${#actual[@]}"
  exit 1
fi

fail=0
for i in "${!expected[@]}"; do
  if [ "${expected[$i]}" != "${actual[$i]}" ]; then
    echo "FAIL: chapter $((i+1)) title mismatch: plot='${expected[$i]}' manuscript='${actual[$i]}'"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "FAIL: manuscript chapter titles do not match the plot chapter titles"
  exit 1
fi

echo "PASS: manuscript chapter titles match the plot chapter titles"
exit 0
