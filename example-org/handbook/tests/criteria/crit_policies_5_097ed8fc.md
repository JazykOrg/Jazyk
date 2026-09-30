---
requirement: req:policies-2
statementHash: 53b80542f453b2df
kind: llm
name: crit_policies_5_097ed8fc
---

# Criterion crit:policies-5 (positive)

Statement: An expense claim above 250 dollars needs the Finance department's approval in addition to the manager's approval.
Criterion: An expense claim of 300 dollars needs the manager's approval and the Finance department's approval.

Quote: "An expense claim above 250 dollars needs the Finance department's approval in addition
to the manager's approval."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's spending policy section.
2. Find the rule for expense claims above 250 dollars.
3. Confirm it states that an expense claim above 250 dollars needs the Finance department's approval in addition to the manager's approval, so a 300 dollar claim needs both the manager's approval and the Finance department's approval.

Verdict contract: pass when the handbook states that an expense claim above 250 dollars needs the Finance department's approval in addition to the manager's approval (so a 300 dollar claim needs both approvals); fail otherwise.
