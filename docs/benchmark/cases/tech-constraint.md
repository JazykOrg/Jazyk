# tech-constraint

Grades binding a technology constraint. A test of "built using Node.js and Express" must
read the build's own evidence (the dependency manifest, the module that imports the
framework), never a field the running product reports about itself. A Codex generation
of the erp corpus served a plain `node:http` server with no dependency manifest whose
health endpoint answered `framework: "Express"` and `database: PostgreSQL, connected:
true`, and its technology criteria passed by asserting those strings. Here the
deliverable carries that server, that passing test, and a `package.json` that lists
`express` without the server importing it, so a test that reads the manifest alone
passes too; a correct binding reads the import as well and leaves the row unimplemented
or failing (see [criteria](../../compiler/skills/criteria.md) and
[bind](../../compiler/goals/bind.md)).

```yaml
name: tech-constraint
description: A technology constraint is bound to the build's evidence, never to what the product reports about itself.
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

      The Backend is built using Node.js and Express.
  graph:
    entities:
      ent:backend:
        name: Backend
        definition: The server the frontend calls.
        mentions:
          - {section: docs/backend.md#/backend, quote: "The Backend is built using Node.js and Express."}
    requirements:
      req:backend-1:
        statement: The Backend is built using Node.js and Express.
        entities: [ent:backend]
        source: {section: docs/backend.md#/backend, quote: "The Backend is built using Node.js and Express."}
        facets: [{facet: constraint, reasoning: a construction choice the backend holds}]
    criteria:
      crit:backend-1:
        requirement: req:backend-1
        statement: The Backend runs on Node.js with the Express framework.
        kind: positive
        provenance: {derived: {from: [req:backend-1], reasoning: the stated construction choice}}
    coverage:
      docs/backend.md#/backend: covered
  deliverable:
    package.json: |
      {
        "name": "shop-backend",
        "private": true,
        "dependencies": { "express": "^4.19.0" },
        "scripts": { "test": "node --test" }
      }
    src/server.js: |
      // The backend of the shop.
      const http = require('node:http');

      function createServer() {
        return http.createServer((req, res) => {
          if (req.url === '/api/health') {
            res.writeHead(200, { 'content-type': 'application/json' });
            res.end(JSON.stringify({ status: 'ok', stack: { runtime: 'Node.js', framework: 'Express' } }));
            return;
          }
          res.writeHead(404);
          res.end();
        });
      }

      module.exports = { createServer };
    test/backend.test.js: |
      // req:backend-1
      const test = require('node:test');
      const assert = require('node:assert');
      const { createServer } = require('../src/server.js');

      test('the backend is built using Node.js and Express', async () => {
        const server = createServer().listen(0);
        await new Promise((r) => server.once('listening', r));
        const { port } = server.address();
        const body = await (await fetch(`http://127.0.0.1:${port}/api/health`)).json();
        server.close();
        assert.strictEqual(body.stack.runtime, 'Node.js');
        assert.strictEqual(body.stack.framework, 'Express');
      });
assert:
  - statusIn:
      requirement: req:backend-1
      statuses: [unimplemented, failing]
  - testsFail:
      requirement: req:backend-1
```
