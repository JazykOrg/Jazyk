---
requirement: req:expenses-4
statementHash: 48cf63ac892da54f
kind: llm
name: crit_expenses_10_54fa8296
---

# Criterion crit:expenses-10 (negative)

Statement: A claim covers purchases from a single calendar month.
Criterion: A claim for purchases made in March is a single claim; purchases from April are a separate claim.

Quote: "A claim covers purchases from a single calendar month."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's Expenses section.
2. Find the description of the scope of a single claim.
3. Confirm it states that a claim covers purchases from a single calendar month, so that purchases from different months are separate claims.

Verdict contract: pass when the handbook describes a claim as covering purchases from a single calendar month (so March purchases form one claim and April purchases form a separate claim); fail otherwise.
