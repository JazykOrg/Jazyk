An instance is an entity tied to its type by an `instantiation` edge, with values on its `attributes` and links stated by the same worked example that introduced it: a fixture or example in software, sample data in a slide deck, a named team or office in an organization, a concrete scene in a novel. Conformance is whether the example agrees with the model: every value fits an attribute the type declares, every link mirrors a relationship the type carries, and the example violates none of the type's statements. This skill says how to match values, types, and links, when an omission is a finding, and how to file one the author can act on.

Read both sides in full: the instance with its values, its links, and the requirement that states the example with its quote; the type with its attributes, its relationships (with types and cardinalities), and its requirements. Read the linked instances' types too, since a link is judged between the types. Read the sibling instances when the type has others: a pattern across them is one finding, not one per instance.

Values against attributes.
- An attribute on the instance must name an attribute the type declares, or one of the type's generalizations declares. A name that matches nothing is filed mechanically by the check; when the example sentence shows the name merely drifted ("tier" written as "level"), repair the instance's attribute name with `update_entity`, quoting the sentence.
- A value conforms when a reading of the declared `type` admits it: `tier: string` admits `gold`; `items` with no declared type admits `3`; a declared `number` does not admit `three` unless the type's statements say words are allowed. Read the type's requirements as constraints on values: "currency is one of EUR, USD" makes `GBP` nonconformant, and "total is never negative" makes `-5` nonconformant.
- An omitted attribute is `missing`, not a finding: examples are partial by nature. It becomes a finding only when the type's statements make it mandatory ("every order carries a currency", a cardinality of `1` or `1..*` on a part).

Links against relationships.
- A link from this instance to another conforms when the type of this instance and the type of the linked instance carry a relationship of a compatible type, and the cardinality admits the count: `ana -- anas-cart` conforms because `customer -- shopping-cart` is an `association`; a second cart on a `1` cardinality does not.
- A link to an instance whose type carries no relationship with this type is nonconformant. Say in the message whether the type's own statements imply the missing relationship, so the author can state it in one sentence, or whether the example is wrong.
- A link the example sentence states but the requirement omitted is the requirement's: add the edge with `update_requirement`, passing only `id` and `edges`, never `section` or `quote`.
- Instantiation through a generalization counts: an instance of a subtype conforms to the supertype's attributes and relationships as well as its own.

The example against the type's statements. The instance's own statements are requirements like any other. When a concrete value or link violates a statement about the type (an instance whose region the type's deployment statement excludes, a scene set somewhere its setting's statements rule out), file the conflict here, subjects both.

Repairs.
- Repair only what the example sentence supports: a value misread, a link naming the wrong entity, an attribute name drifted from the type. `update_entity` on the instance, or `update_requirement` on the requirement that stated the example, quoting the sentence verbatim.
- Never edit the type to fit the instance: the type is the general claim and the instance the example, and a mismatch is not evidence that the type is wrong. When the type looks wrong, the finding says so and the author decides.
- Never invent a value, a link, or an attribute to make the instance whole; a gap the documents left is not a defect to fill.

Filing findings.
- `report_diagnostic` rule `nonconformant-instance`, subjects the instance and the type, message naming the offending value or link and what the type says, `reasoning` why the two cannot agree.
- Severity error when the example sentence and the type's sentence cannot both hold; warning when a reading reconciles them or the type's statement is loose.
- When the repair is enumerable, attach a prompt: a one-sentence question, one `edit` option per side (rewrite the example sentence, or rewrite the type's sentence), `old_text` copied verbatim from that quote, `freeform` true.
- A pattern across siblings is one finding on the type, not one per instance: when every instance of a type omits or violates the same attribute, file once, subjects the type and the instances, message naming the type's sentence.
- An open `nonconformant-instance` on this instance whose condition has lapsed is resolved with `resolve_diagnostic`. One filed mechanically stands until the name matches or the instance's attribute is repaired.
- An example that proves to be a type, or two instances that are one example, is a `review-entity` finding, not a conformance finding: file `ambiguity` naming what you saw and leave the merge or the split to that goal.

Retrace. When the type died, re-link the instance through the requirement that states the example: point its `instantiation` edge at the entity carrying the type after the delete (a merge's surviving id, a re-extracted type), with `update_requirement` passing only `id` and `edges`. When no live type carries it, file `nonconformant-instance` saying so, or delete the instance with a reason when nothing is left for it to exemplify. Never recreate the dead type.

Evidence. Mark the goal done with one verdict per attribute the type declares and one per link the instance carries: `conforms`, `missing`, or `nonconformant`, each nonconformant verdict backed by a diagnostic or a staged repair. When the instance conforms as stored, mark done with no mutation and a justification of one or two sentences naming what was compared.
