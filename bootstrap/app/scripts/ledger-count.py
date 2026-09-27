# Count a generation ledger's requirements and how many pass: a requirement passes
# when it has recorded verdicts and every one is `pass`. Prints "<passing> <rows>".
# Used by refresh-demos.sh and check-demos.sh (docs/frontends/wasm.md#keeping-the-demos-current).
import re, sys

rows, passing = 0, 0
verdicts = None
# The indent of a block scalar being read (an `evidence: |` holds a judge's reply,
# which may carry a verdict line of its own), or None outside one.
block = None
for line in open(sys.argv[1], encoding="utf-8"):
    indent = len(line) - len(line.lstrip(" "))
    if block is not None:
        if not line.strip() or indent > block:
            continue
        block = None
    if re.match(r"^\s*[^\s#].*: [|>][-+0-9]*\s*$", line):
        block = indent
    elif re.match(r"^  req:", line):
        if verdicts is not None:
            passing += bool(verdicts) and all(v == "pass" for v in verdicts)
        rows += 1
        verdicts = []
    elif verdicts is not None:
        # A verdict is a bare or a quoted scalar.
        m = re.match(r"^\s+verdict: ['\"]?(\w+)['\"]?\s*$", line)
        if m:
            verdicts.append(m.group(1))
if verdicts is not None:
    passing += bool(verdicts) and all(v == "pass" for v in verdicts)
print(passing, rows)
