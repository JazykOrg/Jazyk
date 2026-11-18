---
name: flow-views
description: Author behavior diagrams using sourced order, paths and participants.
---
# Behavior diagrams

Load the requirements, sources and existing diagram before editing. Choose one
question: use-case for actor goals, activity for process paths, sequence for
ordered interactions, communication for connections with numbered interactions,
state for lifecycle, timing for stated time constraints, overview for linked phases.
Do not create automatic twins. Additional kinds need a distinct purpose.

Preserve source-supported ordering. Document order is a starting point, not proof
of causality or concurrency. A failure case follows the step it handles. Every
fragment guard and branch cites live requirements. Show only known participants,
messages, transitions and measures. A missing semantic fact is reported rather
than invented in an annotation. Explicit links name real detail or phase diagrams.
Aim for at most 12 steps and 8 sequence participants, with linked detail beyond
those sizes. Follow reported soft and hard limits.

Good: a checkout sequence includes its stated payment failure path with sourced
guards. Bad: invent a retry loop or parallel requests to make the story complete.
Good: a timing diagram explains a stated deadline. Bad: assign guessed timestamps.
