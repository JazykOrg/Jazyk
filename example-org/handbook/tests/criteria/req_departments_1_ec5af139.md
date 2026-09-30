---
requirement: req:departments-1
statementHash: ec5af139fbae58cd
kind: llm
name: crit_departments_25_bb8fd136
---

# Criterion crit:departments-25 (positive)

Statement: Ridgeline Outfitters has four departments: Operations, Sales, Finance, and People.
Criterion: Ridgeline Outfitters' departments are `Operations`, `Sales`, `Finance`, and `People` — four in total.

Quote: "Ridgeline Outfitters has four departments: Operations, Sales, Finance, and People. This
document lists what each one does, the roles inside it, and its teams."

Implementing paths: sales.md

Confirm steps:
1. Open the handbook's Sales page.
2. Find the list of Ridgeline Outfitters' departments.
3. Confirm it names exactly four departments: `Operations`, `Sales`, `Finance`, and `People` — no more, no fewer.

Verdict contract: pass when the handbook lists exactly the four departments `Operations`, `Sales`, `Finance`, and `People` and no others; fail otherwise.
