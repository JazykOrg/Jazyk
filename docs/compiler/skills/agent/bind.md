---
name: bind
description: "Connect existing implementation and test evidence to requirements."
requiredCapabilities: "binding"
---
# Bind existing implementation

Use when asked to connect existing code to documented requirements and criteria.

- Inspect `binding_tasks` for the requested scope. Claim it with `begin_binding`.
- Read the package's requirements, criteria, existing code inventory, and tests.
  Inspect actual implementation and executable evidence with available tools.
- Record observed implementation and test links with `record_binding` using the
  returned hashes and schema. A filename or test name alone proves no coverage.
- Distinguish satisfied, missing, stale, and uncertain behavior. Do not rewrite
  requirements or mark evidence passing merely to make binding succeed.
- For an authorized repair, read `change-code`. For undocumented observed
  behavior, read `reverse-compile`. Those are separate procedures and authority.
- Follow the active package's completion and lease rules. Report the recorded
  bindings and any remaining generation or verification work.
