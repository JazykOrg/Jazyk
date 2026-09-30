---
requirement: req:expenses-8
statementHash: b3185dc40cc500e4
kind: llm
name: crit_expenses_12_63989ef7
---

# Criterion crit:expenses-12 (positive)

Statement: Cost centers use the codes from the ledger, such as `CC-4410` for the Boulder warehouse.
Criterion: The Boulder warehouse's cost center is `CC-4410` from the ledger.

Quote: "Cost centers use the codes from the ledger, for
example `CC-4410` for the Boulder warehouse."

Implementing paths: ledger.md

Confirm steps:
1. Open the handbook's Expenses section, in its worked example.
2. Find where cost centers are assigned to claims.
3. Confirm it states that cost centers use the codes from the ledger, giving `CC-4410` as the code for the Boulder warehouse.

Verdict contract: pass when the handbook states that cost centers use the codes from the ledger, with `CC-4410` as the code for the Boulder warehouse; fail otherwise.
