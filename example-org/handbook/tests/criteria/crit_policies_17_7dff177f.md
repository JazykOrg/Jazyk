---
requirement: req:policies-5
statementHash: 028d6b10a49888b0
kind: llm
name: crit_policies_17_7dff177f
---

# Criterion crit:policies-17 (boundary)

Statement: Every store and the warehouse runs a fire drill once per quarter.
Criterion: A store that runs exactly one fire drill in a quarter meets the requirement; a store that runs none in a quarter does not.

Quote: "Every
store and the warehouse runs a fire drill once per quarter."

Implementing paths: policies.md (Safety section)

Confirm steps:
1. Open policies.md.
2. Find the Safety section.
3. Confirm it states that a store running exactly one fire drill in a quarter meets the requirement, and a store running none in a quarter does not.

Verdict contract: pass when the handbook states that a store running exactly one fire drill in a quarter meets the requirement and a store running none does not; fail otherwise.
