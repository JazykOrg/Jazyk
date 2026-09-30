---
requirement: req:expenses-15
statementHash: c9910249cae4cca2
kind: llm
name: crit_expenses_14_b6e3066e
---

# Criterion crit:expenses-14 (positive)

Statement: Finance reimburses an approved claim in the next payroll run.
Criterion: An approved expense claim is paid in the next payroll run, which runs on the 25th of the month.

Quote: "5. Finance reimburses an approved claim in the next payroll run."

Implementing paths: expenses.md (step 5: Finance reimburses an approved claim in the next payroll run), expense-claim.md

Confirm steps:
1. Open the handbook's Finance / expenses section.
2. Find the description of how Finance handles an approved expense claim.
3. Confirm it states that Finance reimburses an approved claim in the next payroll run, and that the payroll run is the monthly run on the 25th.

Verdict contract: pass when the handbook describes Finance as reimbursing an approved expense claim in the next payroll run (the monthly run on the 25th); fail otherwise.
