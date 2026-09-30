---
requirement: req:expenses-3
statementHash: 96f2019254886f0e
kind: llm
name: crit_expenses_9_5c33c204
---

# Criterion crit:expenses-9 (positive)

Statement: An expense claim carries one receipt for every line.
Criterion: An expense claim with two lines carries two receipts, one per line.

Quote: "An expense claim carries one receipt
for every line."

Implementing paths: expense-claim.md

Confirm steps:
1. Open the handbook's Expenses section.
2. Find the description of how an expense claim carries receipts.
3. Confirm it states that an expense claim carries one receipt for every line, so a claim with two lines carries two receipts, one per line.

Verdict contract: pass when the handbook describes an expense claim as carrying one receipt for every line (a two-line claim carrying two receipts, one per line); fail otherwise.
