---
requirement: req:expenses-13
statementHash: f20556a1462e39a7
kind: llm
name: crit_expenses_5_82240745
---

# Criterion crit:expenses-5 (positive)

Statement: An expense claim with a total above 500 dollars needs manager approval followed by Finance approval.
Criterion: An expense claim of 620 dollars is approved by the manager and then by Finance.

Quote: "| above 500 dollars     | the manager, then Finance        |"

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's Expenses section.
2. Find the description of how the approvals a claim needs depend on its total.
3. Confirm it states that a claim of 620 dollars (above 500 dollars) is approved by the manager and then by Finance.

Verdict contract: pass when the handbook describes a claim above 500 dollars as needing the manager's approval followed by Finance's approval; fail otherwise.
