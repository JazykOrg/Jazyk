---
requirement: req:hiring-26
statementHash: 820f015270ae5dbd
kind: llm
name: crit_hiring_23_91c30066
---

# Criterion crit:hiring-23 (boundary)

Statement: An offer stays open for 7 calendar days.
Criterion: An offer is open on the 7th calendar day after it is made and closed on the 8th calendar day.

Quote: "An offer stays open for 7
calendar days."

Implementing paths: offer.md (Timing section)

Confirm steps:
1. Open the handbook's offer page, the "Timing" part.
2. Find the description of how long an offer stays open.
3. Confirm it states that an offer stays open for 7 calendar days, so it is open on the 7th calendar day after it is made and closed on the 8th calendar day.

Verdict contract: pass when the handbook states that an offer stays open for 7 calendar days (open on the 7th calendar day, closed on the 8th); fail otherwise.
