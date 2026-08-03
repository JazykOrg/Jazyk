# turn-steps

Grades code-block extraction: a fenced pseudo-code block is a claim about the system,
step by step, and each behavioral step yields a requirement quoting that step's own
line. The real-world gap this samples: a model can score well on prose extraction and
still stall on a document whose substance sits inside a fence (the sort-utility
fixture's failure mode). See [ears](../../compiler/concepts/ears.md#code-blocks-state-obligations)
and [case format](../cases.md#case-format).

````yaml
name: turn-steps
description: Extract one requirement per behavioral step of a fenced pseudo-code block.
par:
  rounds: 8
task:
  type: reconcile-doc
  target: docs/dedupe.md
given:
  docs:
    docs/dedupe.md: |
      # Dedupe

      The dedupe utility removes repeated lines from its input.

      ## Steps

      This is pseudo code:

      ```
      Read lines from STDIN one by one:
          Trim whitespace from the current line
          If the line was already seen, skip it
          Otherwise remember the line and print it
      ```
assert:
  - entityExists:
      name: dedupe utility
  - requirementExists:
      earsPattern: 'trim|whitespace'
      entity: dedupe utility
  - requirementExists:
      earsPattern: 'skip|already seen|duplicate'
      entity: dedupe utility
  - requirementExists:
      earsPattern: 'print|remember'
      entity: dedupe utility
  - coverageSet:
      section: 'docs/dedupe.md#/dedupe/steps'
      state: covered
````
