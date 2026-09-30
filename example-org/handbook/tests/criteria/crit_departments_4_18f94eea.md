---
requirement: req:departments-5
statementHash: 0f5c9294ce33e0bf
kind: llm
name: crit_departments_4_18f94eea
---

# Criterion crit:departments-4 (boundary)

Statement: Every department is organized into one or more teams.
Criterion: A department with one team satisfies the organization; a department with no teams does not.

Quote: "Every department is organized into one or more teams."

Implementing paths: (none yet — the handbook is not written)

Confirm steps:
1. Open the handbook's Departments section.
2. Find the description of how departments are organized into teams.
3. Confirm it states that every department is organized into one or more teams.
4. Verify the boundary: a department with exactly one team satisfies the organization, and a department with zero teams does not.

Verdict contract: pass when the handbook states that every department is organized into one or more teams, with the boundary that one team suffices and zero teams does not; fail otherwise.
