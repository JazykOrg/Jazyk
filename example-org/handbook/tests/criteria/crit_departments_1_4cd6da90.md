---
requirement: req:departments-21
statementHash: 15e903bcfe130b60
kind: llm
name: crit_departments_1_4cd6da90
---

# Criterion crit:departments-1 (positive)

Statement: An Account Manager manages wholesale accounts and negotiates their pricing.
Criterion: An Account Manager responsible for a wholesale account has that wholesale account assigned for management and can negotiate its pricing.

Quote: "- Account Manager: manages wholesale accounts and negotiates their pricing."

Implementing paths: wholesale-account.md

Confirm steps:
1. Open the handbook's Wholesale Account page.
2. Find the Account Manager role.
3. Confirm it states the Account Manager manages wholesale accounts and negotiates their pricing.

Verdict contract: pass when the handbook's Sales section describes the Account Manager role as managing wholesale accounts and negotiating their pricing; fail otherwise.
