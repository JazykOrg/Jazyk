# turn-declarative

Grades declarative extraction. The document states obligations in plain declarative
prose, with no `shall` anywhere. A capable model recognizes the obligations, rephrases
them into EARS statements, and keeps the verbatim quotes. See
[declarative prose states obligations](../../compiler/concepts/ears.md#declarative-prose-states-obligations).

```yaml
name: turn-declarative
description: Extract obligations stated as declarative prose, rephrased into EARS.
task:
  type: reconcile-doc
  target: docs/ledger.md
given:
  docs:
    docs/ledger.md: |
      # Ledger

      ## Entries

      The Ledger records every Transaction as an immutable Entry. An Entry never
      changes after it is written.

      ## Balances

      The Ledger recomputes an Account balance from its entries on every read.
assert:
  - entityExists:
      name: Ledger
  - entityExists:
      name: Entry
  - requirementExists:
      earsPattern: 'shall'
      entity: Ledger
  - requirementExists:
      earsPattern: 'never change|immutable|shall not (change|be changed)'
      entity: Entry
  - coverageSet:
      section: 'docs/ledger.md#/ledger/entries'
      state: covered
  - coverageSet:
      section: 'docs/ledger.md#/ledger/balances'
      state: covered
```
