---
purpose: "Notifies the conversation that reopened tool context invalidates previous graph handles."
used_by:
  - "bootstrap/src/conversation.rs"
# Context: graph handles belong to a tool context. After reopening, the old identifiers do not establish that their loaded objects still exist.
---
The tool context was reopened. Prior handles are invalid. Reload only the graph
nodes needed for this request. This does not require task discovery or compilation.
