---
requirement: req:backend-4
criterion: crit:backend-3
name: crit_backend_3_e345b9de
kind: llm
---

# crit_backend_3_e345b9de

Requirement: The backend system provides endpoints for the frontend interface.

Criterion: The Frontend can access Backend endpoints provided for the frontend interface.

Source quote: "It is backed by a PostgreSQL database and provides endpoints for the [frontend interface](./frontend.md)."

Implementing paths inspected:
- app.js
- src/WarehouseApp.tsx

Confirm steps:
1. Inspect `app.js` and confirm it exposes `GET /api/frontend/endpoints` and other JSON endpoints intended for frontend access.
2. Inspect `src/WarehouseApp.tsx` and confirm the frontend presents or uses backend endpoint information.
3. Pass only if the deliverable contains runtime implementation files showing backend endpoints that the frontend can access.

Verdict contract: return PASS when backend endpoint implementation and frontend access are present in runtime source files; otherwise return FAIL with the missing evidence.
