# Done before the goals are marked

The embedded agent on f2 called `done` five times with its goal neither done nor failed.
The refusal names the goal and the two calls that settle it.

```yaml
name: done-open-goal
description: done with a goal neither marked done nor failed is refused naming the goal
goal: {kind: reconcile-section, target: docs/shop.md}
given:
  docs:
    docs/shop.md: |
      # Shop

      The Shop sells Products to Customers.
steps:
  - call: done
    args: {summary: nothing to do}
    expect: {rule: open-goal, contains: "mark_goal_done or mark_goal_failed"}
assert:
  - entityCount: {max: 0}
```
