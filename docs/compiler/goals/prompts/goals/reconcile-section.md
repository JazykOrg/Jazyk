---
purpose: "Reconciliation contract for the reconcile-section goal. The board supplies its target, change, hints, and completion gate."
used_by:
  - "bootstrap/src/goals/reconcile_section.rs"
---
This goal reconciles one section or whole-file input.

## Resolve the goal

For a section of kind `blob`, compile the whole input file.

Its raw body is a compiler-generated envelope containing the path, size, byte digest, and incoming references.

It is not file text and must never be quoted as provenance.

Read the referring sections and existing requirements to decide what the file is for.

Requirements about it quote those documents, never the envelope or invented image text.

Mark it covered with a note naming the requirement IDs whose quoted references select this input.

Supplied assets retain their original bytes.

Generation uses referencedAssets and the existing delivery ledger to preserve or copy them into the required destination; an already correct copy is retained.

Mark an input non-normative only with a concrete reason explaining why its context imposes no obligation.

Missing context is not permission to invent a role.

Blob bytes and reference context changes require reassessment.

Do not extract requirements from envelope fields.

The prose extraction contract below applies to other section kinds.

This goal brings the graph in line with one section whose text changed or was never processed: the loaded body shows the section with its diff marked, the requirements already sourced from it, and the stale anchors on it.

Apply the extraction skill to every sentence, list item, code block step, and test case: what the subject or one of its parts is, does, contains, shows, uses, allows, requires, or limits is a requirement, recorded with upsert_requirement as one atomic statement in clear wording of your own, with the entities it names, its edges, transition, and facets where the sentence states them, and the source sentence as a verbatim quote (for a list item, that single bullet line); an attribute the sentence states goes on the entity too, through the attributes of upsert_entity or update_entity, each one quoting that sentence: a typed property without a value ("each scene has a point of view") is the attribute alone, and a stated value ("Ines is 31") is the attribute with its value beside the requirement that carries it, since the box draws the value and the pair judges it.

A sentence that marks, sets, or puts the subject in a state records a transition beside the statement, and the event that creates the subject names its initial state.

A statement already extracted and still made by the section is left alone; a fact another section already yielded and this section restates is recorded again as this section's own requirement quoting this section (the natural key is section plus statement, so the call mints a sibling, never re-anchors the earlier one), and rejudge-pair then judges the pair; a statement whose sentence was reworded is re-recorded through upsert_requirement (the same statement with a fresh verbatim quote updates it in place); a statement whose meaning changed is revised with update_requirement carrying the new statement and quote; a statement the section does not make any more is deleted with delete_requirement.

Stale anchors are a contract: for each one listed, re-record it when the document still states the fact, revise it when the fact changed, delete it when the fact is gone; an untouched anchor parks the goal and drops the section's coverage mark with it.

Search before creating an entity, reuse under another wording with an alias, mint only what statements are directly about, record a stereotype, parent, or attribute only when the section states it, and read "the system" as the root system, the same way in every document: the entity the front door names as the product (the «system» stereotype, Orderly, the Warehouse ERP), never the page's own subject (an Order does not reserve Stock, the system does on the Order's behalf), and never a second entity minted under the document's own heading.

The page subject (the primarySubject when the loaded set names one, else the entity the document's title names) is what the page's own noun, "it", and "this component" refer to; a statement the system makes about the page subject lists both.

The primarySubject is also this document's subject under its own noun, singular or plural, so alias or update it, never mint a sibling.

The one exception is the extraction skill's area rule: when the entity is an area named by a plural (Orders, Returns) and this document defines the singular thing ("An Order is a customer's request"), the thing is a concept of its own, minted with the area as its parent, never an alias of the area; every other concept the area's page defines takes the area as its parent too, unless a sentence places it elsewhere.

Record the facts as stated, even when sections disagree: judging contradictions and duplicates belongs to the rejudge-pair and review-entity goals that follow, and report_diagnostic is not in this goal's toolset.

Then call set_coverage for the section exactly once, after its extraction: covered when a requirement sourced from it is staged or already recorded; non-normative is the exception, allowed only when no sentence passed the test (navigation that only links elsewhere, a glossary that only defines terms whose rules the component pages own, a changelog, a roadmap wish list; a section that declares itself non-authoritative and defers to the pages that own its facts is this case, and its definitions are not extracted twice), and it needs a real note; a body that is only a lead-in to the child sections ("The system supports the following operations:" above headings, not bullets) is this case too, with the note naming the children that carry the facts.

These three reasons for non-normative are always wrong and are rejected: "it states a fact, not a requirement", "it describes content or appearance, not behavior", "it is not a requirement on the system".

A code block of steps is covered only when each behavioral step has a requirement (the block belongs to the section holding it, so its steps quote that section and its mark covers the block), and a test-case section is never non-normative: its case, and any example sentence with concrete values, is a criterion of the requirement it exercises, recorded with upsert_criterion quoting the case's lead-in or the example sentence (the rule itself is the requirement, recorded first when no statement states it), and the criterion sourced from the section covers it.

The coverage note and the goal justification account for each normative sentence by the requirement that carries it, by id when the id is known and by the statement's opening words when the mark rides in the same reply as the upserts that mint the ids: a justification claiming an extraction names a statement staged in this session or already recorded, and claiming a sentence extracted while staging nothing for it is a dishonest mark.

Every line marked + in the body is new since the last reconcile, and the requirements already sourced here were extracted before it existed, so they never carry it: extract what each added line states (a lead-in that says how the content after it is written or laid out is a requirement on that layout), or, when an added line truly states nothing, repeat it in the coverage note and say why.

When the section already yielded everything it states, stage nothing, set the mark, and mark the goal done.

## Completion gate

The gate: a coverage mark is staged or recorded for the section, every stale anchor on it is addressed, every staged quote locates in the section it names, a covered claim has a requirement sourced from the section behind it, and every added line is overlapped by a quote staged in this session or repeated in the coverage note.

For a whole-file blob, a coverage note cannot substitute for a quoted reference.

If an existing requirement in an incoming referring section quotes only a list lead-in, use `update_requirement` to re-anchor it within that same section, including the linked item that selects this blob.

Other cross-document writes remain forbidden.

Invalid blob coverage is refused before staging; if provenance cannot be repaired, mark the goal failed and save without a coverage claim.