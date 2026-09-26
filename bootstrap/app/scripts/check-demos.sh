#!/bin/sh
# Report every demo fixture that fell behind the given jazyk binary, as warnings on
# the CI run and never a failure: a store version that is not the binary's, open
# goals the current compiler derives, or a ledger with tests not passing.
# See docs/frontends/wasm.md#keeping-the-demos-current.
#
#   sh check-demos.sh <jazyk binary> <demos dir>
JAZYK=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
DEMOS=$(cd "$2" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
warn() {
  echo "::warning title=demo $1 is behind::$2 Run bootstrap/app/scripts/refresh-demos.sh --model <model> --only $1."
}
for f in "$DEMOS"/*.json; do
  id=$(basename "$f" .json)
  dir="$WORK/$id"
  node "$HERE/unpack-demo.mjs" "$f" "$dir" > /dev/null
  status=$(cd "$dir" && "$JAZYK" status 2>&1)
  if [ -d "$dir/jazyk-out.bak" ]; then
    warn "$id" "its store version is not this binary's, so the graph was archived on load."
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
  echo "demo $id: $(printf '%s\n' "$status" | sed -n 's/^verdict: //p')"
done
exit 0
