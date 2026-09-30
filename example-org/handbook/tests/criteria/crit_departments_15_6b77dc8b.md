---
requirement: req:departments-10
statementHash: f1ab65f54c115a60
kind: llm
name: crit_departments_15_6b77dc8b
---

# Criterion crit:departments-15 (positive)

Statement: Operations receives every shipment from a supplier, counts it in, and moves it to the store that ordered it.
Criterion: A shipment a supplier sends is counted in by Operations and moved to the store that ordered it.

Quote: "Operations receives every
shipment from a supplier, counts it in, and moves it to the store that ordered it."

Implementing paths: supplier.md

Confirm steps:
1. Open the handbook's supplier page (supplier.md).
2. Find the description of how Operations handles shipments.
3. Confirm it states that Operations receives every shipment from a supplier, counts it in, and moves it to the store that ordered it.

Verdict contract: pass when the handbook describes Operations as receiving every shipment from a supplier, counting it in, and moving it to the store that ordered it; fail otherwise.
