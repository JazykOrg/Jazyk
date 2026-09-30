---
requirement: req:policies-3
statementHash: 17e5d0fd26ad00cb
kind: llm
name: crit_policies_6_4e72d1bb
---

# Criterion crit:policies-6 (negative)

Statement: The Finance department pays no expense claim without a receipt for every line.
Criterion: An expense claim that is missing a receipt for any line is not paid by Finance.

Quote: "The Finance department pays no claim without a receipt for
every line."

Implementing paths: expense-claim.md

Confirm steps:
1. Open the handbook's Finance / spending policies section.
2. Find the rule governing when Finance pays an expense claim.
3. Confirm it states that Finance pays no expense claim that is missing a receipt for any line.

Verdict contract: pass when the handbook states that Finance pays no expense claim without a receipt for every line (i.e. a claim missing a receipt for any line is not paid); fail otherwise.
