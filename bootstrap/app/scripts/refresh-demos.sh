#!/bin/zsh
# Rebuild every example the demos come from, with a model, and repack the fixtures:
# compile until the graph converges, generate and verify the product, run the tests,
# redraw every diagram, pack, and run the browser tests. The model steps cost money,
# so this runs only when a person runs it, naming the model.
# See docs/frontends/wasm.md#keeping-the-demos-current.
#
#   refresh-demos.sh --model gpt-5.5 [--only <id>] [--free]
#
# --free skips every model step (compile, gen, test) and only redraws and repacks.
set -u
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
JAZYK="$ROOT/bootstrap/target/release/jazyk"
MODEL=""
ONLY=""
FREE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="$2"; shift 2 ;;
    --only) ONLY="$2"; shift 2 ;;
    --free) FREE=1; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
if [ "$FREE" = 0 ] && [ -z "$MODEL" ]; then
  echo "name the model (--model gpt-5.5), or pass --free to only redraw and repack" >&2
  exit 2
fi
[ -x "$JAZYK" ] || { echo "build the binary first: cd bootstrap && cargo build --release" >&2; exit 2; }

# The demos and the example each packs from (the same list as pack-demos.sh).
typeset -A EXAMPLES
EXAMPLES=(shop bootstrap/example/f1 sort example-sort erp example-erp slides example-slides novel example-novel org example-org)
ORDER=(shop sort erp slides novel org)

summary=()
limited=()
for id in $ORDER; do
  [ -n "$ONLY" ] && [ "$ONLY" != "$id" ] && continue
  dir="$ROOT/${EXAMPLES[$id]}"
  echo "=== $id ($dir)"
  cd "$dir"
  if [ "$FREE" = 0 ]; then
    since=$(mktemp)
    # A build stops at its budget with work left; a few passes finish it.
    for pass in 1 2 3; do
      JAZYK_MODEL="$MODEL" "$JAZYK" compile --quiet | tail -2
      grep -q 'state: converged' jazyk-out/status.yaml && break
    done
    if grep -q '^\[gen\]' jazyk.toml; then
      JAZYK_MODEL="$MODEL" "$JAZYK" gen | tail -3
      JAZYK_MODEL="$MODEL" "$JAZYK" test | tail -3
    fi
    # A rate limit leaves the graph half done: such an example is not packed.
    if [ -d jazyk-out/trace ] && find jazyk-out/trace -name '*.jsonl' -newer "$since" -exec grep -l 'HTTP 429' {} + 2>/dev/null | grep -q .; then
      echo "$id: the model was rate limited; its fixture is left as committed"
      limited+=("$id")
    fi
    rm -f "$since"
  fi
  find jazyk-out/diagrams -name '*.svg' -delete 2>/dev/null
  "$JAZYK" docsgen > /dev/null
  verdict=$(sed -n 's/^verdict: //p' <<< "$("$JAZYK" status 2>/dev/null)")
  tests="-"
  if [ -f jazyk-out/gen/ledger.yaml ]; then
    read passing rows <<< "$(python3 "$ROOT/bootstrap/app/scripts/ledger-count.py" jazyk-out/gen/ledger.yaml)"
    tests="$passing of $rows"
  fi
  cost=$(sed -n 's/^cost: //p' <<< "$("$JAZYK" status 2>/dev/null)")
  if (( ${limited[(Ie)$id]} )); then verdict="rate limited ($verdict)"; fi
  summary+=("$id|$verdict|$tests|$cost")
done

# Pack only what was rebuilt: another example's out directory may hold work in
# progress that its committed fixture should not take.
cd "$ROOT"
packs=()
for id in $ORDER; do
  [ -n "$ONLY" ] && [ "$ONLY" != "$id" ] && continue
  (( ${limited[(Ie)$id]} )) && continue
  packs+=("$id=${EXAMPLES[$id]}")
done
[ ${#packs} -gt 0 ] && node bootstrap/app/scripts/pack-demo.mjs bootstrap/app/public/demos $packs

echo
printf '%-7s %-45s %-12s %s\n' demo verdict tests 'last build cost'
for row in $summary; do
  IFS='|' read -r id verdict tests cost <<< "$row"
  printf '%-7s %-45s %-12s %s\n' "$id" "$verdict" "$tests" "$cost"
done
echo
if [ ${#limited} -gt 0 ]; then
  echo "Rate limited: ${limited[*]}. Run again when the quota is back; nothing of theirs was packed."
  exit 1
fi
echo "Next: cd bootstrap/app && npm run build:demo && PW_CHANNEL=chrome npx playwright test, then commit bootstrap/app/public/demos."
