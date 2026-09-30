---
requirement: req:hiring-13
statementHash: 4801d4a698aab8e4
kind: llm
name: req_hiring_13_4801d4a6
---

# Criterion crit:hiring-4 (positive)

Statement: An application is in the offered stage when an offer letter is with the candidate.
Criterion: An application with an offer letter with the candidate is in the `offered` stage.

Quote: "- offered: an offer letter is with the candidate."

Implementing paths: offer-letter.md (The offered stage section)

Confirm steps:
1. Open the handbook's offer letter page, the "The offered stage" part.
2. Find the `offered` stage definition.
3. Confirm it states that an application with an offer letter with the candidate is in the `offered` stage.

Verdict contract: pass when the handbook defines the `offered` stage as an offer letter being with the candidate; fail otherwise.
