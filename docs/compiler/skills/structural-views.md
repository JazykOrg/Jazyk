A structural view is a set of entities and every relationship among them: a class, object, package, component, composite, or deployment view. Membership is what you decide; arrows are never yours. Every arrow comes from a relationship derived from requirement edges, so a missing arrow is a missing statement or a missing edge (`declare-edges` work), never something a view can add. This skill says what belongs in each kind, how to collapse, how lifting keeps a coarse view true, and how to resolve a limit, in any medium: a class view of an organization is its org chart, a package view of a deck is its sections, a component view of a novel is its settings and the characters who move between them.

Membership per kind.
- Class: the entities of one scope or one neighborhood, with their typed attributes. Instances belong in object views, not class views; an entity with attribute values is an instance. Actors appear when statements tie them to the members.
- Object: the instances of one type or of one worked example, named `instance : Type` through their `instantiation` edge, with their attribute values. Links come from `association` groups among them.
- Package: the containers, each holding its children; only entities with children belong. Relationships crossing package boundaries lift to the packages.
- Component: the parts of one system: the children of a containment root, the interfaces they realize (drawn as lollipops), the entities depending on those interfaces (drawn as sockets), and the actors among them. Interface-like means labeled `interface` or realized by something.
- Composite: one entity, the boundary; the emitter draws its children as parts and the connectors among them and across the boundary. One member only.
- Deployment: only what deployment statements state ("The shop is deployed in the EU region"); never synthesize topology from containment or from assumptions.
- A state view is the derived machine of its one member. There is nothing to curate but the subject.

Query or list. Prefer a `query` (`scope`, `parent`, `stereotype`, `depth`) when the rule is a scope, a subtree, or a label: new matches arrive as `query-match` goals and the view stays current. Prefer a member list when the view is hand-picked. When a query match does not belong, exclude it with a note rather than switching to a list; the note is the reason the next build reads, and an excluded node is not asked about again.

Collapse and lifting.
- `collapse` shows an entity with children as one node. Its hidden subtree's relationships lift to it, and the collapsed node links to the sub-view detailing it: the view of the same kind whose `query.parent` is the collapsed entity, or whose members are its children.
- Lifting is render-time aggregation over `parent` chains: a relationship touching a hidden descendant lifts to the nearest shown ancestor, and lifted or collapsed arrows show the strongest type with a count. Nothing is stored, so a coarse view stays true without listing leaves.
- Collapse a subtree whose internals are another view's business (a service in a system view, a chapter in a book view, a department in a company view). Keep expanded what the view exists to show.
- Never hide an entity to hide an arrow. An arrow that looks wrong is a statement that looks wrong: read the contributing requirements (the arrow's justification) and file a diagnostic against the statement.

Limits. Members per structural view: 20 soft, 30 hard. Edges per view: 40 soft, 60 hard. Instances per object view: 15 soft, 25 hard. Past a hard threshold the view renders with its largest subtrees auto-collapsed and a visible note until `split-view` resolves it. Resolve a limit in this order, with reasoning at each step:
- Collapse the subtrees whose internals the view does not need; each collapsed node gets a sub-view.
- Exclude members that belong to another view, with notes.
- Split into sub-views along the structure the documents state: one per child container, one per scope, one per stereotype (the actors and interfaces in one component view, the internals in another). Each sub-view is `upsert_view` with a title naming what it details and a `query` or a member list; the parent view collapses the entity the sub-view details, which is the link.
- Never satisfy a limit by silently omitting an arrow, and never split along a structure the documents do not state. When the members have no containment to split along, the pressure is the container's, not the view's: fail the goal naming the entity, and `abstract-entity` on it is the resolution.

Default views. A default view recomputes at every commit while its rule holds (a class view per scope, a component view per system, an object view per type, a state view per subject). Any edit through `update_view` (a retitle, an exclusion, a collapse, a member change) makes it curated: the recompute leaves it alone from then on, and new candidates reach it only as curation goals through its `query`. Deleting a default view is refused; exclude or collapse instead.

Retrace. A member that died is dropped with `remove_members`, or pointed at its redirect target after a merge; a collapsed entity that died leaves `collapse`; a view with no members left is deleted with `delete_view` and the reason. Default views recompute at commit and are never retraced by hand.

Justify in one or two sentences: what the view exists to show, why each collapse or exclusion, which stated structure a split follows.
