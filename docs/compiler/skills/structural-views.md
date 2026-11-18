---
name: structural-views
description: Author structural diagram membership and presentation intent.
---
# Structural diagrams

Load the diagram, its area and relevant facts before editing. Name one reader's
question. Use class for concepts and attributes, object for concrete instances,
component for systems and interfaces, package for stated group boundaries,
composite for one whole and connected parts, deployment for known placements.

Keep one abstraction level. Show outside interactors when they explain the boundary.
Use explicit area, ordered members, query, exclusions and collapse deliberately.
Selected relationships must exist; omission needs a reason. Labels and notes cannot
invent domain facts. Presentation packages do not reparent entities. Use linked
views for detail. Prefer at most 20 structural members and 40 arrows; respect
reported limits and split with no fact lost rather than truncating silently.

Good: show a customer, the shop and the stated payment provider in system context.
Bad: place every domain class and deployment host in that same context picture.
Good: show known instance values in an object snapshot. Bad: guess sample values.
