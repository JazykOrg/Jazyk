---
requirement: req:expenses-18
statementHash: a8d1c7cbb558a602
kind: llm
name: crit_expenses_20_5bcbe502
---

# Criterion crit:expenses-20 (boundary)

Statement: The Manager reviews an Expense Claim within 5 business days of submission.
Criterion: An expense claim reviewed on the 5th business day after submission is within the limit; a review on the 6th business day is late.

Quote: "The manager reviews a claim within 5 business days of submission."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's Expenses section, its timing subsection.
2. Find the stated limit for a manager's review of a claim.
3. Confirm the limit is 5 business days, so a review on the 5th business day is within the limit and a review on the 6th business day is late.

Verdict contract: pass when the handbook's stated limit is 5 business days (a 5th-business-day review is within the limit, a 6th-business-day review is late); fail otherwise.
