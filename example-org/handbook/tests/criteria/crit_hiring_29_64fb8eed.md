---
requirement: req:hiring-24
statementHash: d91f035d3da860e6
kind: llm
name: crit_hiring_29_64fb8eed
---

# Criterion crit:hiring-29 (boundary)

Statement: The recruiter completes the phone screen within 5 business days of the application.
Criterion: A phone screen completed on the 5th business day after the application is within the window; one completed on the 6th business day is past it.

Quote: "The recruiter completes the phone screen within 5 business days of the application."

Implementing paths: application.md (Timing section)

Confirm steps:
1. Open the handbook's application page, the "Timing" part.
2. Find the description of the 5 business day window for completing the phone screen.
3. Confirm the window is inclusive of the 5th business day and exclusive of the 6th business day.

Verdict contract: pass when the handbook states that a phone screen completed on the 5th business day after the application is within the window and one completed on the 6th business day is past it; fail otherwise.
