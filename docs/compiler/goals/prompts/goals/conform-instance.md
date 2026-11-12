---
purpose: "Reconciliation contract for the conform-instance goal. The board supplies its target, change, hints, and completion gate."
used_by:
  - "bootstrap/src/goals/conform_instance.rs"
---
This goal checks one instance against its type after either changed: an instance is an entity tied to its type by an instantiation edge, its values on attributes, its links stated by the same worked example that introduced it.

## Resolve the goal

Load the instance, its type, and the linked instances' types in full and compare with the conformance skill: every attribute value on the instance names an attribute the type (or a generalization of it) declares and fits its declared type and the type's own statements; every link from the instance (its outgoing edges other than the instantiation edge, as the links hint lists them; an inbound composition or aggregation from a container is the container's and takes no verdict) mirrors a relationship between its type and the linked instance's type, with the cardinality admitting the count, or, when the linked entity is no instance (a character, a place), realizes an attribute the type declares (a scene's setting is a place, its point of view a character), named in the verdict; an omitted attribute is missing, not a finding, unless the type's statements make it mandatory (a cardinality of 1 or 1..* on a part).

The mechanical check already files an attribute name the type does not declare; this goal owns values, links, omissions, and the example against the type's statements.

When the example sentence supports a repair (a value misread, a link naming the wrong entity, an attribute name drifted from the type), conform the instance with update_entity, or add the edge the sentence states to the requirement that stated the example with update_requirement passing only id and edges, never section or quote.

When the documents themselves disagree (a value the type does not allow, a link the type does not have), file report_diagnostic rule nonconformant-instance, subjects the instance and the type, message naming the offending value or link and what the type says, severity error when the two sentences cannot both hold and warning otherwise, with a prompt offering the edit on either sentence when the repair is enumerable; a pattern shared by every sibling instance is one finding on the type, not one per instance.

Never edit the type to fit the instance: the type is the general claim and the instance the example, and a mismatch is not evidence that the type is wrong.

Never invent a value, a link, or an attribute to make the instance whole; a gap the documents left is not a defect to fill.

No creation, deletion, or merge here: an example that proves to be a type is filed with report_diagnostic rule ambiguity, and two instances that are one example with rule duplicate-entity, subjects naming the entities, and the merge or split is left to the type's review-entity goal.

An open nonconformant-instance diagnostic on this instance whose condition has lapsed is resolved with resolve_diagnostic.

When the instance conforms as stored, mark the goal done with no mutation and a justification naming what was compared.

## Completion gate

The gate: mark_goal_done carries evidence {attributes: [{name, verdict}], links: [{to, verdict}]} with one verdict per attribute the type declares, inherited ones included, and one per link the instance carries (conforms, missing, or nonconformant), every nonconformant verdict backed by a staged or open nonconformant-instance diagnostic naming the instance or the type or by a staged repair, and every open nonconformant-instance on the instance either backed by a verdict or resolved.