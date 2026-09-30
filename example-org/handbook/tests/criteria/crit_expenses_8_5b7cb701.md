---
requirement: req:expenses-24
statementHash: 084e309e34733f19
kind: llm
name: crit_expenses_8_5b7cb701
---

# Criterion crit:expenses-8 (positive)

Statement: An Expense Claim is paid in the Payroll run on the 25th.
Criterion: Dana's approved claim of 620 dollars is paid in the payroll run on the 25th.

Quote: "The claim is paid in the payroll run on the 25th."

Implementing paths: expenses.md (Example: Dana's 620-dollar claim is paid in the payroll run on the 25th), expense-claim.md

Confirm steps:
1. Open the handbook's Expenses section, in its worked example.
2. Find the example of Dana, a sales associate, filing a claim for 620 dollars after a trade show.
3. Confirm the example states the claim is paid in the payroll run on the 25th.

Verdict contract: pass when the handbook's example states that Dana's approved 620-dollar claim is paid in the payroll run on the 25th; fail otherwise.
