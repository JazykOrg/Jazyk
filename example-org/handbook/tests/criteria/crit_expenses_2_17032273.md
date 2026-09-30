---
requirement: req:expenses-9
statementHash: e4a5794d16f2d6c4
kind: llm
name: crit_expenses_2_17032273
---

# Criterion crit:expenses-2 (positive)

Statement: The employee submits the expense claim to their manager with a receipt attached to every line.
Criterion: A claim with a receipt on every line is submitted to the employee's manager; a claim with a line missing its receipt is not.

Quote: "1. The employee submits the expense claim to their manager with a receipt attached to
   every line."

Implementing paths: expense-claim.md

Confirm steps:
1. Open the handbook's Expenses section, the steps describing how an expense claim is submitted.
2. Find the description of the employee submitting the claim to their manager.
3. Confirm it states that the employee submits the expense claim to their manager with a receipt attached to every line, and that a claim with a line missing its receipt is not submitted.

Verdict contract: pass when the handbook describes the employee submitting the expense claim to their manager with a receipt attached to every line, and that a claim with a line missing its receipt is not submitted; fail otherwise.
