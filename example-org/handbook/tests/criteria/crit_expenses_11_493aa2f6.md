---
requirement: req:expenses-6
statementHash: a005307e45f994d4
kind: llm
name: crit_expenses_11_493aa2f6
---

# Criterion crit:expenses-11 (positive)

Statement: Claims are filed on form EX-12 in the expense tool.
Criterion: A claim is filed on form `EX-12` in the expense tool.

Quote: "Claims are filed on form EX-12 in the expense tool."

Implementing paths: expense-tool.md

Confirm steps:
1. Open expense-tool.md.
2. Find the description of how a claim is filed.
3. Confirm it states that claims are filed on form EX-12 in the expense tool.

Verdict contract: pass when the handbook describes claims as being filed on form EX-12 in the expense tool; fail otherwise.
