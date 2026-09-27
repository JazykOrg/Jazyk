# gen-node

Grades the conventions a generated deliverable follows, over a Node package: the
dependency manifest pins every version (never `latest` or `*`), the package is named for
the entity that owns it, and the deliverable has exactly one entry point. A paid
generation of the erp corpus pinned every dependency to `latest`, named the package that
held the backend `warehouse-erp-frontend`, and wrote two entry points (`server.js` and
`app.js`, each requiring express and pg) with every test green, so nothing but the
manifest and the file tree can grade these. The fixture is one entity whose document
states a package that depends on nothing external, so the case runs offline with no
install: `node --test` is the runner and the pin rule is graded as no `latest` or `*`
version string in `package.json`. The behavior checks are the ones
[gen-basic](./gen-basic.md) applies: a row per requirement, the recorded tests pass, and
the greeting's test fails once the harness breaks the product. See
[case format](../cases.md#case-format) and the
[conventions](../../consumers/gen.md#file-ownership-and-conventions) the contract
states.

```yaml
name: gen-node
description: A generated Node package pins its dependencies, is named for its owning entity, and has one entry point.
tier: generation
par:
  rounds: 12
goal:
  kind: generate
  target: ent:greeter
given:
  docs:
    docs/greeter.md: |
      # Greeter

      The Greeter is an npm package. It exports a function `greet(name)` that returns
      `Hello, <name>!`. Its `npm test` script runs the tests with `node --test`. The
      package depends on no external package.
  graph:
    entities:
      ent:greeter:
        name: Greeter
        definition: An npm package that greets by name.
        mentions:
          - section: docs/greeter.md#/greeter
            quote: The Greeter is an npm package.
    requirements:
      req:greeter-1:
        statement: The Greeter shall be an npm package.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: The Greeter is an npm package.
      req:greeter-2:
        statement: The Greeter shall export a function `greet(name)` that returns `Hello, <name>!`.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: It exports a function `greet(name)` that returns
      req:greeter-3:
        statement: The Greeter's `npm test` script shall run the tests with `node --test`.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: Its `npm test` script runs the tests with `node --test`.
      req:greeter-4:
        statement: The Greeter shall depend on no external package.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: The package depends on no external package.
    coverage:
      docs/greeter.md#/greeter: covered
assert:
  - generationRecorded: {}
  - rowPerRequirement: {}
  - testsPass: {}
  - testFalsifiable:
      requirement: req:greeter-2
      replace: 'Hello, '
  # The manifest exists and names the package for the entity.
  - fileMatches:
      path: package.json
      pattern: '"name"\s*:\s*"[^"]*greeter[^"]*"'
      min: 1
  # No version string is latest or * (an empty manifest of dependencies passes too).
  - fileMatches:
      path: package.json
      pattern: '"\s*:\s*"(latest|\*)"'
      max: 0
  # No external dependency, as the document states.
  - fileMatches:
      path: package.json
      pattern: '"dependencies"\s*:\s*\{\s*"'
      max: 0
  # npm test runs node --test.
  - fileMatches:
      path: package.json
      pattern: '"test"\s*:\s*"node --test'
      min: 1
  # The manifest names the entry point.
  - fileMatches:
      path: package.json
      pattern: '"(main|exports)"\s*:'
      min: 1
  # Exactly one module that is not a test: one entry point, no duplicate of it.
  - fileCount:
      pathPattern: '\.(js|mjs|cjs)$'
      excludePattern: '(^|/)(test|tests|__tests__|node_modules)/|\.(test|spec)\.(js|mjs|cjs)$'
      min: 1
      max: 1
```
