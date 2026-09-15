# A justification that is an essay

The embedded agent on f2 met eight `bad-justification` refusals for three- and
four-sentence justifications. The gate holds at two sentences and says so.

```yaml
name: justification-essay
description: A four-sentence justification is refused as an essay, naming the sentence count
goal: {kind: reconcile-section, target: docs/shop.md}
given:
  docs:
    docs/shop.md: |
      # Shop

      The Shop sells Products to Customers.
steps:
  - call: set_coverage
    args: {section: docs/shop.md#/shop, state: covered}
  - call: mark_goal_done
    args:
      goal: g:reconcile-section:docs/shop.md
      justification: The section states one fact. I extracted it as an entity. The coverage is marked. Nothing else remains here.
    expect: {rule: bad-justification, contains: "4 sentences"}
assert:
  - entityCount: {max: 0}
```
