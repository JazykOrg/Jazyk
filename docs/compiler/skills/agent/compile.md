---
name: compile
description: "Compile documentation into the semantic graph and resolve reconciliation goals."
requiredCapabilities: "compile orchestration or goal loop"
---
# Compile documentation

Use when the user asks to compile, reconcile, or build the semantic graph.

- Compilation resolves docs into the graph. It does not by itself generate or
  deploy the deliverable. Check explicit authority and project cost rules.
- Use `compile_document_sets` when available for an internal compile. Omit
  `setIds` for all dirty sets, or pass the requested IDs. This can start model
  sessions. Do not launch it as a side effect of answering or editing prose.
- When acting as the compiler worker, or when the serving offers the goal loop,
  call `goals`, then `begin_goals` for the next ready or assigned batch. Follow
  the returned contract, skills, loaded context, and gate.
- Use `load`, `expand`, `unload`, and `graph_status` to manage context. Stage
  findings with the allowed tools. Search before creating graph facts.
- Resolve each goal with `mark_goal_done` and evidence or a one-line
  justification as required. Use `mark_goal_failed` for a concrete failure.
- Call `done` to validate and commit the batch. Follow its result. Continue only
  within the requested compile scope. Use `abandon_goals` to release unfinished
  work when it cannot continue.
- An `awaiting release` result is a gate. Use `release_work` only when the user
  authorized that stage and the tool is available. Human decisions and paid
  actions still need their own authority. Never edit the control files.
- If this serving cannot run compilation, identify the available CLI or UI build
  path. Do not claim that merely reading this skill enables a build.
- Report the actual verdict, remaining mandatory goals, diagnostics, parked
  work, and cost evidence. Do not claim convergence from a successful tool call.
