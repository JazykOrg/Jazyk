#!/bin/sh
# Redraw every committed demo fixture's diagrams with the given jazyk binary and pack
# them back in place. Free and deterministic: no model runs, only the renderer.
# The deploy runs it on dist-demo/demos, so a renderer change reaches jazyk.dev
# without a rebuild of the examples. See docs/frontends/wasm.md#keeping-the-demos-current.
#
#   sh rerender-fixtures.sh <jazyk binary> <demos dir>
set -e
JAZYK=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
DEMOS=$(cd "$2" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
for f in "$DEMOS"/*.json; do
  id=$(basename "$f" .json)
  dir="$WORK/$id"
  node "$HERE/unpack-demo.mjs" "$f" "$dir" > /dev/null
  # A drawing is redrawn when it is missing, so remove them all.
  find "$dir/.jazyk/diagrams" -name '*.svg' -delete 2>/dev/null || true
  (cd "$dir" && "$JAZYK" docsgen > /dev/null)
  node "$HERE/pack-demo.mjs" "$DEMOS" "$id=$dir"
done
