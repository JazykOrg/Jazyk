# Count a generation ledger's requirements and how many pass: a requirement passes
# when it has recorded verdicts and every one is `pass`. Prints "<passing> <rows>".
# Used by refresh-demos.sh and check-demos.sh (docs/frontends/wasm.md#keeping-the-demos-current).
import re, sys

rows, passing = 0, 0
verdicts = None
for line in open(sys.argv[1], encoding="utf-8"):
    if re.match(r"^  req:", line):
        if verdicts is not None:
            passing += bool(verdicts) and all(v == "pass" for v in verdicts)
        rows += 1
        verdicts = []
    elif verdicts is not None:
        m = re.match(r"^\s+verdict: (\w+)", line)
        if m:
            verdicts.append(m.group(1))
if verdicts is not None:
    passing += bool(verdicts) and all(v == "pass" for v in verdicts)
print(passing, rows)
