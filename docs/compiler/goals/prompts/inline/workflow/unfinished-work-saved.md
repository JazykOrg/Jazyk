---
purpose: Distinguish saved staging from committed graph work when a workflow stops.
consumers: workflow compile result after listing open persisted attempts
inputs: One positional Rust format argument, the count of saved unfinished attempts.
---
{} unfinished attempt(s) remain saved for recovery in project chat. They are not committed graph changes. Ask the agent to inspect saved work and resume a suitable attempt before starting fresh.
