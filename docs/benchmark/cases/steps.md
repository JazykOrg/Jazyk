# steps

Grades code-block extraction: a fenced pseudo-code block is a claim about the system,
step by step, and each behavioral step yields a requirement quoting that step's own
line. The step checks match statements alone and one check grades the entity's name, so a
naming slip (the heading's "Dedupe" for the sentence's "dedupe utility") costs that
check, never the extraction the case is about. The real-world gap this samples: a model can score well on prose extraction and
still stall on a document whose substance sits inside a fence (the sort-utility
fixture's failure mode). See [code blocks state obligations](../../compiler/concepts/statements.md#code-blocks-state-obligations)
and [case format](../cases.md#case-format).

````yaml
name: steps
description: Extract one requirement per behavioral step of a fenced pseudo-code block.
par:
  rounds: 8
goal:
  kind: reconcile-section
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
      statementPattern: 'trim|whitespace'
  - requirementExists:
      statementPattern: 'skip|already seen|duplicate'
  - requirementExists:
      statementPattern: 'print|remember'
  - coverageSet:
      section: 'docs/dedupe.md#/dedupe/steps'
      state: covered
````
