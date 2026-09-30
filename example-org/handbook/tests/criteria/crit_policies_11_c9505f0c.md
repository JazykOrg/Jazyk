---
requirement: req:policies-10
statementHash: b72e489c22659ad6
kind: llm
name: crit_policies_11_c9505f0c
---

# Criterion crit:policies-11 (positive)

Statement: Policy numbers such as POL-101 are intranet filing labels and are not part of the rule.
Criterion: A policy's intranet number such as `POL-101` is a filing label and does not change the rule it labels.

Quote: "Each policy has a number on the intranet
(POL-101 and so on); the number is a filing label, not part of the rule."

Implementing paths: policies.md

Confirm steps:
1. Open policies.md.
2. Find how policy numbers such as POL-101 are described.
3. Confirm it states that a policy's intranet number (such as POL-101) is a filing label and does not change the rule it labels.

Verdict contract: pass when the handbook states that a policy's intranet number such as POL-101 is a filing label and is not part of the rule it labels; fail otherwise.
