---
requirement: req:expenses-16
statementHash: b89bedb8f86d58f4
kind: llm
name: crit_expenses_24_aa695f4c
---

# Criterion crit:expenses-24 (boundary)

Statement: If a receipt is missing, the manager returns the claim to the employee, and the employee resubmits it with the receipt within 10 business days.
Criterion: An expense claim resubmitted with its receipt on the 10th business day after the return is within the window; one resubmitted on the 11th business day is past it.

Quote: "6. If a receipt is missing, the manager returns the claim to the employee, and the
   employee resubmits it with the receipt within 10 business days."

Implementing paths: expense-claim.md

Confirm steps:
1. Open the handbook's Expenses section.
2. Find the step describing the resubmission window for a claim returned for a missing receipt.
3. Confirm the window is 10 business days: a resubmission on the 10th business day is within the window and one on the 11th is past it.

Verdict contract: pass when the handbook states the resubmission window as 10 business days (10th day within, 11th day past); fail otherwise.
