---
requirement: req:hiring-26
statementHash: 820f015270ae5dbd
kind: llm
name: crit_hiring_22_51beb44c
---

# Criterion crit:hiring-22 (quality)

Statement: An offer stays open for 7 calendar days.
Criterion: An offer made on a Monday is still open 7 calendar days later, on the following Monday.

Quote: "An offer stays open for 7
calendar days."

Implementing paths: offer.md (Timing section)

Confirm steps:
1. Open the handbook's offer page, the "Timing" part.
2. Find the description of how long an offer stays open.
3. Confirm it states that an offer stays open for 7 calendar days, so an offer made on a Monday is still open 7 calendar days later, on the following Monday.

Verdict contract: pass when the handbook states that an offer stays open for 7 calendar days (so an offer made on a Monday is still open on the following Monday); fail otherwise.
