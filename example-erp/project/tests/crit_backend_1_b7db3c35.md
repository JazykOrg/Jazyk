---
requirement: req:backend-1
criterion: crit:backend-1
name: crit_backend_1_b7db3c35
kind: llm
---

# LLM criterion: crit_backend_1_b7db3c35

Statement: The backend system is a REST API.

Criterion: A consumer of the Backend sees the Backend exposed as a REST API.

Source quote: "The backend system is a REST API built using Node.js and Express."

Implementing paths inspected:
- app.js

Confirm steps:
1. Inspect `app.js` and confirm it creates an Express application with JSON middleware.
2. Confirm `app.js` exposes HTTP resources such as `GET /`, `GET /subsystems`, `GET /api/frontend/endpoints`, and inventory routes.
3. Pass only if the delivered backend is accessible as a REST-style HTTP API.

Verdict contract: PASS when source files implement an accessible REST-style HTTP API; FAIL when no backend API implementation exists.
