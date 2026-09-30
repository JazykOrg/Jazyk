---
requirement: req:expenses-7
statementHash: f21c94b8251350b9
kind: llm
name: crit_expenses_25_13910977
---

# Criterion crit:expenses-25 (positive)

Statement: Scanned receipts are stored under `S:\Finance\Claims\<year>\<employee>`.
Criterion: A scanned receipt for a 2024 claim by employee `Dana` is stored under `S:\Finance\Claims\2024\Dana`.

Quote: "Scanned receipts are stored under
`S:\Finance\Claims\<year>\<employee>`."

Implementing paths: expense-claim.md

Confirm steps:
1. Open the handbook's expense-claim section.
2. Find the description of where scanned receipts are stored.
3. Confirm it states that scanned receipts are stored under `S:\Finance\Claims\<year>\<employee>`, so that a 2024 claim by employee `Dana` is stored under `S:\Finance\Claims\2024\Dana`.

Verdict contract: pass when the handbook states that scanned receipts are stored under `S:\Finance\Claims\<year>\<employee>` (so a 2024 claim by `Dana` lands under `S:\Finance\Claims\2024\Dana`); fail otherwise.
