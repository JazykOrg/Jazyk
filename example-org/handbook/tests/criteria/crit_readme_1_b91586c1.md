---
requirement: req:readme-9
statementHash: 588980e237efa2a4
kind: llm
name: crit_readme_1_b91586c1
---

# Criterion crit:readme-1 (positive)

Statement: Every Ridgeline Outfitters department has a head.
Criterion: Each of the four departments `Operations`, `Sales`, `Finance`, and `People` has exactly one head.

Quote: "Every department has a head."

Implementing paths: README.md (The company section)

Confirm steps:
1. Open README.md, the "The company" section.
2. Find the description of each of the four departments: Operations, Sales, Finance, and People.
3. Confirm that each of the four departments is stated to have exactly one head.

Verdict contract: pass when the handbook states that each of the four departments (Operations, Sales, Finance, and People) has exactly one head; fail otherwise.
