# The conform-instance goal

`conform-instance` checks one instance against its type after either changed. An
instance is an entity tied to its type by an `instantiation` edge
([edges](../model/requirement.md#edges)), its values on `attributes`, its links stated
by the same worked example that introduced it: a fixture in software, sample data in a
deck, a named team in an organization, a scene in a novel. The session compares values
against the attributes the type declares, links against the relationships the type
carries, and the example against the type's statements. Where the example sentence
supports a repair it conforms the instance; where the documents disagree it files
`nonconformant-instance`. It never edits the type: the type is the general claim and the
instance the example, and a mismatch is not evidence that the type is wrong. The
mechanical half (attribute names against the type's declarations) is a
[check](../compilation.md#checks); this goal is the judgment half.

- Class: compile. Mandatory. Readiness tier 2.
- Unit: one instance entity. Goal id `g:conform-instance:ent:<slug>`, e.g.
  `g:conform-instance:ent:ana`.
- Skill: [`conformance`](../skills/conformance.md).

## Created when

The goal derives from an `instance-changed` [change record](../graph.md#change-records)
on the instance. A commit writes one when the instance or the model under it changed,
`via` naming how:

- `edges`: an `instantiation` edge landed on a requirement (a new instance), or the
  requirement stating the example was revised (a link added or removed, a value
  restated).
- `attributes`: the instance's values changed, or the type's attribute names or types
  changed.
- `entities`: a requirement on the type was created, revised, or deleted; the type's
  statements constrain the values ("currency is one of EUR, USD").
- `recompute`: a relationship of the type was added, removed, or retyped at commit
  ([recompute](../model/relationship.md#recompute)), so a link's backing changed.
- `parent`: the type gained or lost a generalization; an instance conforms to the
  supertype's attributes and relationships as well as its own.

`detail` names the type and what changed. One record per instance: a change on a type
writes one for each of its instances, so a type with many worked examples fans out to
that many goals, batched together. The commit that resolves a `conform-instance` goal
writes no `instance-changed` record for its target from that session's own repairs;
the session saw the result it staged.

E.g.:

```yaml
- id: c414-1
  generation: 414
  mutation: 1
  kind: instance-changed
  subject: ent:ana
  via: attributes
  detail: {type: ent:customer, changed: [attributes.tier.type]}
```

The mechanical conformance check files `nonconformant-instance` for an attribute name
the type does not declare, at the end of every build. Those diagnostics are open when
the session runs; a repair of the name resolves them.

## Readiness

- Tier 2: ready when no tier 0 or 1 goal is open or parked
  ([readiness](../reconciler.md#readiness)). The example's section and the type's
  sections are settled before they are compared.
- Locality is the node neighborhood ([batching](../reconciler.md#batching)): the
  instances of one type batch together, and with the type's `review-entity` goal when
  it is open. A pattern across siblings is then one finding on the type, not one per
  instance.
- A `retrace` on the instance (its `instantiation` requirement deleted) runs in the
  same tier; an instance with no type is repaired or deleted there, not conformed.

## Gate

`mark_goal_done({goal, justification, evidence})` carries `evidence` with one verdict
per attribute the type declares and one per link the instance carries:

```yaml
evidence:
  attributes:
    - {name: tier, verdict: conforms}
    - {name: region, verdict: missing}
  links:
    - {to: ent:anas-cart, verdict: conforms}
    - {to: ent:eu-warehouse, verdict: nonconformant, carried_by: diag:nonconformant-instance-2}
```

The harness validates the claim over the store plus what the session has staged:

- Every attribute the type and its generalizations declare has a verdict; every link on
  the instance (an `association`, `aggregation`, or `composition` edge from the
  instance on the example's requirement) has a verdict. A verdict is `conforms`,
  `missing`, or `nonconformant`.
- `nonconformant` is backed by an open `nonconformant-instance` diagnostic naming the
  instance and the type, staged or recorded, or by a repair staged in this session
  (`update_entity` on the instance, `update_requirement` on the example's
  requirement).
- `missing` is accepted: examples are partial by nature. Whether a missing value
  violates a mandatory whole-part is the model's call, filed as `nonconformant` when it
  does.
- The type is never edited from this goal: an `update_entity` whose `id` is the type,
  or an `update_requirement` on a requirement whose `entities` do not include the
  instance, is rejected naming this rule.
- The justification is present.

`done` runs the same gate over every goal in the batch and the per-mutation gates on
what was staged ([validation gates](../graph.md#validation-gates)): a re-anchored value's
quote locates, `report_diagnostic` subjects exist, a prompt's `old_text` locates in the
section it names. When the instance conforms as stored, the session stages nothing and
marks the goal done with a justification naming what was compared.
`mark_goal_failed({goal, reason})` is for an instance that instantiates two unrelated
types and documents that do not say which governs. A failed goal keeps its record and
surfaces on the instance; it blocks convergence.

## Hints

Computed by the harness and rendered under the goal block:

- The type and what changed, from the record.
- The type's declared attributes with their types, its generalizations, and the
  relationships between the type and the linked instances' types with their
  cardinalities ([types](../model/relationship.md#types)).
- The instance's values and links side by side with the declaration each must match.
- The mechanical findings already filed on the instance (attribute names), and any
  other open `nonconformant-instance` diagnostic on it.
- The sibling instances of the type in the batch, so a pattern is filed once on the
  type.
- `load ent:<instance>`, `load ent:<type>`, `skill conformance`, and the tools:
  `update_entity` on the instance, `update_requirement` on the example's requirement,
  `report_diagnostic` `nonconformant-instance`.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's: the
agent contract, the active skills, the project block, the goals block, the loaded set.
The goal block carries the contract paragraph from
[`prompts/conform-instance.md`](./prompts/conform-instance.md): load the instance and
its type in full and compare; every value names an attribute the type declares with a
compatible type, every link corresponds to a relationship the type has with the linked
entity's type, every mandatory whole-part is satisfied; conform the instance when the
example sentence supports the repair, quoting it verbatim; file
`nonconformant-instance` when the documents disagree, with severity `error` when the
two sentences cannot both hold and `warning` otherwise, and a prompt offering the edit
on either sentence when the repair is enumerable; never edit the type to fit the
instance; never invent a value, a link, or an attribute to make the instance whole;
resolve a lapsed diagnostic; mark done with no mutation when the instance conforms.
Then the change in one line, the gate in one line, and the hints.

The `conformance` skill is active from the first round: values against attributes,
links against relationships, the example against the type's statements, repairs,
filing findings, evidence. Loading an instance brings it anyway
([skills](../sessions.md#skills)).

The initially loaded set for the batch holds:

- The instance in full ([policy](../context.md#policy)): its `attributes` with values
  and provenance, its links, and the requirement that states the example with its
  quote and edges.
- The type in full: its `attributes` with types, its requirements with statements, its
  relationships with types and cardinalities, and its generalizations as stubs.
- The linked instances and their types as stubs.

E.g.:

```
## Goals
- [g:conform-instance:ent:ana] mandatory
  [contract paragraph]
  Change: type ent:customer changed in g414 (attributes.tier.type).
  Gate: a verdict per declared attribute and per link; nonconformant backed by a diagnostic or a repair.
  Hints: type declares tier: string; instance tier = gold; link ent:anas-cart backed by
  customer -- shopping-cart association; no open diagnostic; skill conformance

## Loaded (5.1k/24k chars)
- ent:ana        full: instance of ent:customer; tier = gold; 1 link   (req:shop-9)
- ent:customer   full: attributes tier: string; 3 requirements; 2 relationships
- ent:anas-cart  stub (instance of ent:shopping-cart)
- ent:shopping-cart   stub (definition only)
skills: conformance (active); extraction, judgment, flow-views, structural-views, abstraction (load_skill)
```

`jazyk preview <goal>` renders the prompt before it is spent
([preview](../sessions.md#preview)).

## Tools

The `conform-instance` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- `update_entity({id, attributes?})`: the instance only; a value misread or an
  attribute name drifted from the type, each attribute quoting the example sentence.
- `update_requirement({id, edges?, statement?, quote?})`: the example's requirement
  only; a link the sentence states but the requirement omitted, passing only `id` and
  `edges`; a value restated in the statement with the quote that carries it.
- `report_diagnostic({rule, severity, subjects, message, reasoning, prompt?})`, rule
  `nonconformant-instance`, subjects the instance and the type, one finding per pattern
  across siblings.
- `resolve_diagnostic({id, reason})`: a `nonconformant-instance` diagnostic whose
  condition lapsed.
- [`report_feedback`](../tools.md#feedback-tool).

No creation or delete tools, no coverage, and no view tools: an object view derives
from the `instantiation` edges ([default views](../model/view.md#default-views)), and
its curation is [GC](../graph.md#garbage-collection). An example that proves to be a
type, or two instances that are one example, is not a conformance finding: the session
files `duplicate-entity` or `ambiguity` with `report_diagnostic` naming the entities,
and the type's next [`review-entity`](./review-entity.md) goal judges it with the
statements loaded whole.
