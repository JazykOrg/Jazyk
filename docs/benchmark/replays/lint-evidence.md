# A review that never checked the lint rule

The local model read the quote carrying the planted lint violation and marked the review
done without checking the project's lint rule, scoring `review-lint` 0.50 on both codecs. A
review names every lint rule in its evidence by number: a claim without the entry is
refused as `lint-unchecked`, `filed` without a lint finding on the entity or its
statements as `lint-unbacked`, and the claim holds once the finding is reported.

```yaml
name: lint-evidence
description: A review claim without a lint entry, or filed without a finding, is refused; filed with a lint finding holds
tier: review
goal: {kind: review-entity, target: ent:gadget}
given:
  docs:
    docs/gadget.md: |
      # Gadget

      ## Overview

      The Gadget shall report its battery level every hour (TODO: confirm the interval).
  graph:
    entities:
      ent:gadget:
        name: Gadget
        definition: A battery-powered field device.
        mentions:
          - {section: docs/gadget.md#/gadget/overview, quote: "The Gadget shall report its battery level every hour (TODO: confirm the interval)."}
    requirements:
      req:gadget-1:
        statement: The Gadget shall report its battery level every hour.
        entities: [ent:gadget]
        source: {section: docs/gadget.md#/gadget/overview, quote: "The Gadget shall report its battery level every hour (TODO: confirm the interval)."}
        facets: [{facet: behavior, reasoning: the device reports}]
  lint:
    warnings:
      - A TODO marker appears in prose.
steps:
  - call: mark_goal_done
    args:
      goal: g:review-entity:ent:gadget
      justification: The definition matches the one statement.
      evidence: {definition: current}
    expect: {rule: lint-unchecked, contains: 'rule \[1\]'}
  - call: mark_goal_done
    args:
      goal: g:review-entity:ent:gadget
      justification: The quote carries a TODO marker.
      evidence: {definition: current, lint: [{rule: 1, verdict: filed}]}
    expect: {rule: lint-unbacked}
  - call: report_diagnostic
    args:
      rule: lint
      severity: warning
      subjects: [req:gadget-1]
      message: The quote of req:gadget-1 carries a TODO marker.
  - call: mark_goal_done
    args:
      goal: g:review-entity:ent:gadget
      justification: The quote carries a TODO marker, filed as a lint finding.
      evidence: {definition: current, lint: [{rule: 1, verdict: filed}]}
  - call: done
    args: {summary: the TODO marker filed under lint}
assert:
  - diagnosticExists: {rule: lint}
```
