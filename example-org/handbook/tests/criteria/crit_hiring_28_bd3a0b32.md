---
requirement: req:hiring-23
statementHash: 29c8e8c162de1382
kind: llm
name: crit_hiring_28_bd3a0b32
---

# Criterion crit:hiring-28 (boundary)

Statement: The recruiter tells the candidate about every stage change within 2 business days.
Criterion: A candidate told of a stage change on the 2nd business day is within the window; one told on the 3rd business day is past it.

Quote: "The\nrecruiter tells the candidate about every stage change within 2 business days."

Implementing paths: application.md (Moving between stages section, notification)

Confirm steps:
1. Open the handbook's application page, the "Moving between stages" part.
2. Find the description of the 2 business day window for telling the candidate of a stage change.
3. Confirm the window is inclusive of the 2nd business day and exclusive of the 3rd business day.

Verdict contract: pass when the handbook states that a candidate told of a stage change on the 2nd business day is within the window and one told on the 3rd business day is past it; fail otherwise.
