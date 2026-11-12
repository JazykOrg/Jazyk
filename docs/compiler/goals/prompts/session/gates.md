<!--
Purpose: One completion gate per goal kind. The session selects a named fragment instead of sending the entire bundle.
Used by: bootstrap/src/session/prompt.rs.
This is a fragment bundle: marker names are selected by code, not a sequence of instructions.
-->
<!-- Context: place anchors fragment. This fragment is static; the consumer selects it by name. -->
<!-- place-anchors -->
every proposal decided: place_anchor or orphan_anchor staged for each

<!-- Context: reconcile section fragment. This fragment is static; the consumer selects it by name. -->
<!-- reconcile-section -->
a coverage mark staged or recorded; stale anchors addressed; every covered claim honest; every + line accounted for

<!-- Context: rejudge pair fragment. This fragment is static; the consumer selects it by name. -->
<!-- rejudge-pair -->
a verdict for the pair in evidence; for an acting verdict, the mutation or diagnostic that carried it

<!-- Context: review entity fragment. This fragment is static; the consumer selects it by name. -->
<!-- review-entity -->
definition claimed; one backed verdict per listed lookalike, unreferenced statement, open diagnostic, and composition part

<!-- Context: derive criteria fragment. This fragment is static; the consumer selects it by name. -->
<!-- derive-criteria -->
one verdict per listed requirement (complete, stated-only, or open-question) with its criteria in
evidence; every derived criterion carries from, reasoning, and a proposal; no stale criterion
remains, refresh still-valid criteria explicitly or replace or delete them

<!-- Context: retrace fragment. This fragment is static; the consumer selects it by name. -->
<!-- retrace -->
nothing on the target points at a dead or missing node

<!-- Context: conform instance fragment. This fragment is static; the consumer selects it by name. -->
<!-- conform-instance -->
values and links conform or nonconformant-instance filed; one verdict per declared attribute (inherited included) and link in evidence

<!-- Context: bind fragment. This fragment is static; the consumer selects it by name. -->
<!-- bind -->
a current ledger row recorded by record_binding

<!-- Context: generate fragment. This fragment is static; the consumer selects it by name. -->
<!-- generate -->
record_generation landed with a live factHash

<!-- Context: verify fragment. This fragment is static; the consumer selects it by name. -->
<!-- verify -->
a verdict recorded on the row with a live factHash

<!-- Context: ratify fragment. This fragment is static; the consumer selects it by name. -->
<!-- ratify -->
the human accepts the proposal (a dual write) or retracts the fact

<!-- Context: answer fragment. This fragment is static; the consumer selects it by name. -->
<!-- answer -->
the human answers; applying the answer is an answer session

<!-- Context: declare edges fragment. This fragment is static; the consumer selects it by name. -->
<!-- declare-edges -->
typed edges declared, or the justification says the statement is not structural

<!-- Context: dedupe candidates fragment. This fragment is static; the consumer selects it by name. -->
<!-- dedupe-candidates -->
merged, or kept separate with the reasoning

<!-- Context: curate view fragment. This fragment is static; the consumer selects it by name. -->
<!-- curate-view -->
membership decided: one verdict per matched node in evidence, added, or excluded with a note

<!-- Context: split view fragment. This fragment is static; the consumer selects it by name. -->
<!-- split-view -->
the count is within the limit, no member lost, sub-views linked

<!-- Context: abstract entity fragment. This fragment is static; the consumer selects it by name. -->
<!-- abstract-entity -->
sub-entities carry parent and derived provenance, detail moved, docs proposals staged

<!-- Context: unknown fragment. This fragment is static; the consumer selects it by name. -->
<!-- unknown -->
the kind's page states the gate
