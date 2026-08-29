Abstraction is the graph's answer to scale: an entity over its requirement limit (50 soft, 80 hard), over its children limit (10 soft, 20 hard), or a subject whose state machine is over its state limit (12 soft, 20 hard) is split into sub-entities under it, detail moved down, and the documents asked to state the new structure. Lifting then keeps every coarse view true. This skill applies in every medium: a service split into modules, a department into teams, a long chapter's cast into scenes, a deck's section into slides.

Read everything first. Load the entity in full and every requirement on it, gathered across all documents. A GC goal runs only once the cone is quiet, so the counts you see are final for this build. Abstract once, holistically, never in a stream of partial states.

Split by cohesion.
- Group the requirements by what they are about: a sub-concept, a phase, a role, an interface, a lifecycle stage, a recurring noun. A group is a candidate sub-entity when the documents already have a noun for it (a section title, a repeated phrase, a list item) and its statements are about that noun directly.
- Name the sub-entity in the documents' own wording. Never invent a concept the documents cannot support: a group with no noun in the documents is not a sub-entity. When the requirements cohere into no such groups, fail the goal with the reason and let the human decide.
- Respect scopes: a split never crosses a scope. Respect stated containment: where the documents state a whole-part (a `composition` edge), the part's `parent` is that whole, and no sub-entity may contradict it.
- Search before you create. A noun the documents have may already be an entity elsewhere in the tree; a sub-entity that exists is re-parented with `update_entity` `parent`, never minted twice. Two same-named entities under different parents are two concepts, so always pass `parent`.

Build the tree.
- Create each sub-entity with `upsert_entity` and no `mention`, since no sentence states it yet: `parent` the entity, `definition` written as the sentence the documents should gain, in their vocabulary, a `stereotype` when the medium calls for one, and a `note` naming the requirements it groups. It lands with derived provenance: `from` the requirements moved under it in this session, `reasoning` from the note.
- Move the detail: re-point each grouped requirement with `update_requirement`, passing only `id` and `entities`, the sub-entity in place of the parent when the statement is about the part, the parent kept when the statement is about the whole. Keep `transition.subject` and every edge end in `entities`. Attributes move with their concept, through `update_entity` on the sub-entity.
- The parent keeps the statements about the whole and its definition; refresh the definition to name the parts when the documents' wording does.
- Over the children limit, introduce an intermediate parent grouping children by cohesion, the same way: a noun the documents have, the children re-pointed to it with `update_entity` `parent`.
- Over the state limit, the subject conflates phases or concepts. Split the subject into a sub-entity per phase whose states are that phase's, and re-point each transition statement's `entities` and `transition.subject` to the sub-entity whose states it uses. Never merge states to lower the count.

Docs proposals. Every sub-entity with derived provenance is invented until the documents state it. The harness files a ratification proposal per derived fact: the sentence the docs should gain, inserted into the best section, accepted or retracted by a human through the `ratify` goal.
- Your part is the sentence. The `definition` is what the proposal carries, so write it as prose the author would keep, in the documents' vocabulary ("The order service consists of the cart module, the pricing module, and the fulfillment module."), never as graph jargon.
- Prefer a target the documents already structure: the section that introduces the parent, or a new sub-document when the parent's section is already over its size limit. Name the target in the note.
- One proposal per fact, never a bundle: a parent that gains three parts is three sentences, or one sentence naming the three, and the note says which.

Views. After a split, the class, component, and package views that showed the entity keep drawing it: lifting folds the sub-entities' relationships into the parent wherever the parent is shown collapsed, and default views recompute at commit. Collapse the parent in the coarse views that do not need the internals; give the internals a sub-view when they earn one.

Limits and dismissal. When the count is honest and every split would invent structure, do not split. Raising the node's own limit is a human's decree, recorded with decree provenance, and the goal stops deriving until the raised threshold is crossed. Fail the goal with that recommendation in the reason; the failure surfaces on the entity where the human will see it.

Docs split versus graph split. The same pressure can be answered by splitting a section or splitting an entity. A section over its size limit is the author's to split (`section-too-large`); the graph split is yours only when the requirements cohere into sub-concepts the documents name. When the pressure comes from one oversized section that lists everything about one concept, leave the entity whole and fail the goal pointing at the section.

Retrace. When a sub-entity's parent died, re-point `parent` to the nearest live ancestor with `update_entity`, or unset it when none remains; when a merge absorbed the parent, the redirect target is the new parent. When a derived entity's `from` set lost a member, re-derive it: keep it when the surviving requirements still cohere under its noun, saying why; when they do not, move its requirements back to the parent with `update_requirement` and delete it with a reason. A dead node is never recreated here.

Stability. A split re-derived on a later build must land on the same natural keys (the same names under the same parent), so the upserts are no-ops. Never re-split what a compile review merged back: a flip between a GC split and a compile merge parks the pair as `unstable-derivation`, blocked on a human. Never delete the parent, and never move a requirement whose statement is about the whole.

Justify in one or two sentences: the cohesion each sub-entity follows and the sentence in the documents that names it.
