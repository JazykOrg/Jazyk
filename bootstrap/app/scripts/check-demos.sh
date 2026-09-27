#!/bin/sh
# Report every demo fixture that fell behind the given jazyk binary, as warnings on
# the CI run and never a failure: a store version that is not the binary's, open
# goals the current compiler derives, a ledger with tests not passing, or diagrams
# that are not the current renderer's output.
# See docs/frontends/wasm.md#keeping-the-demos-current.
#
#   sh check-demos.sh <jazyk binary> <demos dir>
JAZYK=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
DEMOS=$(cd "$2" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
# warn <id> <what> [<fix>]: the fix is a refresh unless the caller names another.
warn() {
  echo "::warning title=demo $1 is behind::$2 ${3:-Run bootstrap/app/scripts/refresh-demos.sh --model <model> --only $1.}"
}
for f in "$DEMOS"/*.json; do
  id=$(basename "$f" .json)
  dir="$WORK/$id"
  node "$HERE/unpack-demo.mjs" "$f" "$dir" > /dev/null
  status=$(cd "$dir" && "$JAZYK" status 2>&1)
  # A store of another version reads as empty; only a build archives it, so the
  # version is compared here: the fixture's against the one the binary reports.
  packed=$(sed -n 's/^version: *//p' "$dir/jazyk-out/status.yaml" 2>/dev/null)
  binary=$(printf '%s\n' "$status" | sed -n 's/^store: version \([0-9]*\),.*/\1/p')
  if [ "$packed" != "$binary" ]; then
    warn "$id" "its store version is ${packed:-missing}, not this binary's $binary, so its graph reads as empty."
    continue
  fi
  open=$(printf '%s\n' "$status" | sed -n 's/^board: \([0-9]*\) open.*/\1/p')
  if [ -n "$open" ] && [ "$open" -gt 0 ]; then
    warn "$id" "the current compiler derives $open open goal(s) for it."
  fi
  if [ -f "$dir/jazyk-out/gen/ledger.yaml" ]; then
    set -- $(python3 "$HERE/ledger-count.py" "$dir/jazyk-out/gen/ledger.yaml")
    passing=$1
    rows=$2
    if [ "$rows" -gt 0 ] && [ "$passing" -lt "$rows" ]; then
      warn "$id" "$passing of $rows tests pass."
    fi
  fi
  # The renderer skips a view whose committed .puml is its own output and whose .svg
  # exists; a view it draws is one the fixture has behind. Free, no model runs.
  docsgen=$(cd "$dir" && "$JAZYK" docsgen 2>&1)
  drawn=$(printf '%s\n' "$docsgen" | sed -n 's/.*diagrams: \([0-9]*\) drawn.*/\1/p')
  if [ -n "$drawn" ] && [ "$drawn" -gt 0 ]; then
    warn "$id" "$drawn of its diagram(s) are not the current renderer's output." "Run bootstrap/app/scripts/rerender-fixtures.sh <jazyk> bootstrap/app/public/demos (or refresh-demos.sh --free)."
  fi
  echo "demo $id: $(printf '%s\n' "$status" | sed -n 's/^verdict: //p')"
done
exit 0
