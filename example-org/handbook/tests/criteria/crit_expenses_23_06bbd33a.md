---
requirement: req:expenses-16
statementHash: b89bedb8f86d58f4
kind: llm
name: crit_expenses_23_06bbd33a
---

# Criterion crit:expenses-23 (positive)

Statement: If a receipt is missing, the manager returns the claim to the employee, and the employee resubmits it with the receipt within 10 business days.
Criterion: When a manager returns an expense claim for a missing receipt, the employee resubmits it with the receipt within 10 business days.

Quote: "6. If a receipt is missing, the manager returns the claim to the employee, and the
   employee resubmits it with the receipt within 10 business days."

Implementing paths: expense-claim.md

Confirm steps:
1. Open the handbook's Expenses section.
2. Find the step describing what happens when a receipt is missing.
3. Confirm it states that the manager returns the claim to the employee and the employee resubmits it with the receipt within 10 business days.

Verdict contract: pass when the handbook describes the manager returning a claim with a missing receipt to the employee and the employee resubmitting it with the receipt within 10 business days; fail otherwise.
