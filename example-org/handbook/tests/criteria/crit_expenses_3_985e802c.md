---
requirement: req:expenses-11
statementHash: da0bbfdd3619fbac
kind: llm
name: crit_expenses_3_985e802c
---

# Criterion crit:expenses-3 (boundary)

Statement: Expense claim approvals depend on the claim total.
Criterion: An expense claim of 500 dollars needs the manager's approval only, while a claim of 501 dollars needs the manager's and Finance's approval.

Quote: "3. The approvals a claim needs depend on its total:"

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's Expenses section.
2. Find the description of how the approvals a claim needs depend on its total.
3. Confirm it states that a claim of 500 dollars needs the manager's approval only, while a claim of 501 dollars needs the manager's and Finance's approval.

Verdict contract: pass when the handbook describes the approval threshold as 500 dollars or less needing the manager's approval only, and above 500 dollars needing the manager's approval followed by Finance's approval; fail otherwise.
