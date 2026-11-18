# The curate-view goal

Review an authored structural diagram's query matches. The query already expands
membership at commit. The model keeps each matched entity or excludes it with a
reason. No query creates a diagram.

- Kind: `curate-view`. Class: GC. Optional.
- Unit: one view. ID: `g:curate-view:<view id>`.
- Ready when its compile cone has no open or parked work.

## Created when

A `query-match` change names matches not recorded by the view's
`graph/decisions/curation/` decision. Each view has one goal covering its matches.
Completion records all current query matches, so only later matches ask again.
Interaction coverage is reviewed by [diagram-coverage](./diagram-coverage.md).

## Gate

The model inspects the view's purpose, area and existing members before choosing.
Keep a match in membership or save an exclusion with a concrete reason. Queries
and exclusions retain their ordinary validation gates. Delete a diagram only
when its explanation no longer serves a purpose, with a reason.

## Hints

Load the authored diagram and each query match. Existing relationships and source
references provide the evidence. Diagram guidance follows the target's kind.

## What the model sees

The session receives the diagram in full and matched entities in full. Load
related requirements and source sections on demand. Preserve authored grouping,
labels, annotations, relationship choices and links while curating membership.

## Tools

Read with `load`, `expand`, `unload`, `graph_status`, `search`, `read_section`,
`get_entity`, `get_view`, `diagnostics`, `inspect_input`, `list_views` and
`list_diagram_suppressions`. Write through `upsert_view`,
`update_view`, and `delete_view`. `mark_goal_done` records a justified outcome;
`done` validates and commits the batch atomically. Suppression tools are available
only when the batch also includes diagram coverage work.

`mark_goal_failed` records a concrete failure. `load_skill` supplies diagram
guidance. `report_feedback` and `report_spend` record session observations.
