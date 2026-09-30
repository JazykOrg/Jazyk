---
requirement: req:departments-19
statementHash: d340c43cb010b38f
kind: llm
name: crit_departments_28_5bdd701f
---

# Criterion crit:departments-28 (positive)

Statement: Sales has Sales Associate as a role.
Criterion: The build's structure shows Sales holding the Sales Associate role.

Quote: "- Sales Associate: helps customers on the floor and rings up sales at the register."

Implementing paths: sales.md

Confirm steps:
1. Open the handbook's Sales page.
2. Find the list of roles under Sales.
3. Confirm that Sales Associate is listed as a role of Sales.

Verdict contract: pass when the handbook's Sales department section lists Sales Associate as a role; fail otherwise.
