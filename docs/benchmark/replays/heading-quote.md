# Heading quoted as body

A Codex session on the novel corpus quoted the section heading the loaded section had
rendered (`### Chapter 1: Arrival`) and read the generic absence line as a contradiction
(D143). A quote equal to the heading is refused as a title, with the reason.

```yaml
name: heading-quote
description: A quote that is the section's heading is refused as a title, with the reason
goal: {kind: reconcile-section, target: docs/plot.md}
given:
  docs:
    docs/plot.md: |
      # Plot

      ## Chapter 1: Arrival

      Theo Brandt arrives in Port Alder on the afternoon ferry.
steps:
  - call: upsert_entity
    args:
      name: Chapter 1
      definition: The first chapter of the manuscript.
      mention: {section: docs/plot.md#/plot/chapter-1-arrival, quote: "## Chapter 1: Arrival"}
    expect: {rule: quote-not-found, contains: heading}
  - call: upsert_entity
    args:
      name: Chapter 1
      definition: The first chapter of the manuscript.
      mention: {section: docs/plot.md#/plot/chapter-1-arrival, quote: "Chapter 1: Arrival"}
    expect: {rule: quote-not-found, contains: heading}
assert:
  - entityAbsent: {namePattern: "^Chapter 1$"}
```
