# gen-basic

Grades the generation session end to end: the model gets a fixture graph (one entity,
three requirements naming a small shell deliverable) and an empty temp deliverable. It
must write the product and its tests with the file tools, record an honest manifest,
and produce falsifiable tests. The falsifiability check is the heart: the harness runs
each recorded command as recorded (must pass), then replaces the mandated exact string
in the product files and runs the designated requirement's command again (must fail).
A test that passes either way scores nothing. See
[case format](../cases.md#case-format).

```yaml
name: gen-basic
description: Generate a small shell deliverable with an honest manifest and falsifiable tests.
tier: generation
par:
  rounds: 10
goal:
  kind: generate
  target: ent:greeter
given:
  docs:
    docs/greeter.md: |
      # Greeter

      The Greeter is a POSIX shell script named hello.sh. Running it prints exactly
      `Hello, Jazyk!` to stdout and exits 0. Running it with `-q` prints nothing and
      exits 0.
  graph:
    entities:
      ent:greeter:
        name: Greeter
        definition: A POSIX shell script that greets.
        mentions:
          - section: docs/greeter.md#/greeter
            quote: The Greeter is a POSIX shell script named hello.sh.
    requirements:
      req:greeter-1:
        statement: The Greeter shall be a POSIX shell script named hello.sh.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: The Greeter is a POSIX shell script named hello.sh.
      req:greeter-2:
        statement: The Greeter shall print exactly `Hello, Jazyk!` to stdout and exit 0.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: Running it prints exactly
      req:greeter-3:
        statement: When run with `-q`, the Greeter shall print nothing and exit 0.
        entities: [ent:greeter]
        source:
          section: docs/greeter.md#/greeter
          quote: Running it with `-q` prints nothing and
    coverage:
      docs/greeter.md#/greeter: covered
assert:
  - generationRecorded: {}
  - rowPerRequirement: {}
  - testsPass: {}
  - testFalsifiable:
      requirement: req:greeter-2
      replace: 'Hello, Jazyk!'
```
