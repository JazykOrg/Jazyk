# The diagram-coverage goal

Review one area's visual explanation after its requirements and structure settle.
The model loads its facts and existing diagrams, chooses a useful kind, and saves
an authored diagram or a reasoned suppression. Mandatory review blocks only its
area. Optional advice does not block implementation. Explicit obligations cannot
be silently suppressed. The [plan](../../../plans/authored-diagrams.md) defines
kind selection, initial thresholds, schema, and test scenarios.

## Gate

Every finding is covered by live compatible diagrams or a current suppression.
The gate recomputes membership and source support. An empty diagram never passes.
Model completion claims and staged mutations commit atomically.

## Tools

Read with `load`, `expand`, `unload`, `graph_status`, `search`, `read_section`,
`get_entity`, `get_view`, `diagnostics`, `list_views`, and
`list_diagram_suppressions`. Save with `upsert_view`, `update_view`, `delete_view`,
`suppress_diagram_coverage`, and `unsuppress_diagram_coverage`. Complete or fail
with `mark_goal_done`, `mark_goal_failed`, and `done`; `load_skill` loads guidance.
`report_feedback` and `report_spend` record session observations.

`inspect_input` reads the identity and source sections of referenced input.

## Source diagrams

A source PlantUML section opens a source-backed diagram obligation after semantic
extraction. Diagram-review tools author its quoted view. Extraction itself keeps
its existing semantic tools. A heuristic suppression cannot waive this obligation.

## Evidence for suggestions

Interaction candidates use directional dependency edges, transitions, and existing
criteria with scenario conditions. They never inspect requirement classifications.
Stateful subjects with measured criteria can receive an optional timing review.
The model confirms whether the bound concerns time. Criteria may arrive after
initial diagram review and open a later optional review. No count requires a
speculative timing diagram.
