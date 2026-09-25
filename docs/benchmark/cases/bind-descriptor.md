# bind-descriptor

Grades a bind session facing a descriptor. The deliverable holds `src/backend.js`, an
object literal naming a framework and an endpoint list, and a passing test that imports
it and asserts its fields. Nothing serves `GET /products`. The object describes the
backend in another syntax; it is not the backend, and the test that reads it drives
nothing. The session must not bind that test as the requirement's evidence: it rewrites
the test in place so it drives the artifact (starts a server and sends the request),
and the row reads `unimplemented` (no file carries the requirement) or `failing`. The
harness reruns every recorded test: one that still passes against this deliverable reads
a description. A generated ERP once read verified on 42 of 42 criteria with no server,
because its tests asserted descriptor objects (see
[the bind goal](../../consumers/bind.md#the-bind-goal)). Running the case needs `node`
on the path.

```yaml
name: bind-descriptor
description: A bind session never binds a test that reads a descriptor object as the evidence a requirement is met.
tier: generation
par:
  rounds: 8
goal:
  kind: bind
  target: req:backend-1
given:
  docs:
    docs/backend.md: |
      # Backend

      The Backend is a REST API that lists the products at `GET /products`.
  graph:
    entities:
      ent:backend:
        name: Backend
        definition: The REST API that serves the product list.
        mentions:
          - {section: docs/backend.md#/backend, quote: "The Backend is a REST API that lists the products at `GET /products`."}
    requirements:
      req:backend-1:
        statement: The Backend is a REST API that lists the products at `GET /products`.
        entities: [ent:backend]
        source: {section: docs/backend.md#/backend, quote: "The Backend is a REST API that lists the products at `GET /products`."}
        facets: [{facet: behavior, reasoning: the backend answers a request}]
    criteria:
      crit:backend-1:
        requirement: req:backend-1
        statement: A client sends `GET /products` to the running Backend and receives the list of products in the response.
        kind: positive
        provenance: {derived: {from: [req:backend-1], reasoning: the observation a client makes of the stated endpoint}}
    coverage:
      docs/backend.md#/backend: covered
  deliverable:
    package.json: |
      {
        "name": "shop",
        "private": true,
        "scripts": {
          "test": "node --test"
        }
      }
    src/backend.js: |
      // The backend of the shop.
      module.exports = {
        style: 'REST',
        framework: 'express',
        endpoints: ['/products'],
      };
    test/backend.test.js: |
      // req:backend-1
      const test = require('node:test');
      const assert = require('node:assert');
      const backend = require('../src/backend.js');

      test('the backend is a REST API that lists the products', () => {
        assert.strictEqual(backend.style, 'REST');
        assert.strictEqual(backend.framework, 'express');
        assert.ok(backend.endpoints.includes('/products'));
      });
assert:
  - statusIn:
      requirement: req:backend-1
      statuses: [unimplemented, failing]
  - testsFail:
      requirement: req:backend-1
```
