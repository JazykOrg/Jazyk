# paragraph-layout

Grades a prescribed layout beside its content. Extracting each sentence loses the
number and grouping of the paragraphs. The layout lead-in is an independent
requirement. See [enumerations](../../compiler/concepts/statements.md#enumerations).

```yaml
name: paragraph-layout
description: Keep the three ordered paragraph groups beside the sentence-content obligations.
goal:
  kind: reconcile-section
  target: docs/landing.md
given:
  docs:
    docs/landing.md: |
      # Landing Page

      ## Hero

      The Landing Page hero copy is written as separate paragraphs:

      > Alpha describes the compiler. Beta describes its tests.

      > Gamma describes editing.

      > Delta describes the IDE. Epsilon describes diagnostics.
assert:
  - entityExists:
      name: Landing Page
  - requirementExists:
      statementPattern: '(?i)(three|3).*paragraph|paragraph.*(three|3)'
      entity: Landing Page
  - requirementExists:
      statementPattern: '(?is)Alpha.*Beta.*Gamma.*Delta.*Epsilon'
      entity: Landing Page
  - coverageSet:
      section: 'docs/landing.md#/landing-page/hero'
      state: covered
```
