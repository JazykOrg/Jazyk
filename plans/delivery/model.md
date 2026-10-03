# Delivery representation

Status: proposal. This document describes responsibilities and information that
must survive. Field names and storage paths are illustrative, not a final schema.
Read with [options](./options.md) and the [workflow](./workflow.md).

## Semantic and operational records

The semantic graph describes obligations. A delivery record describes observations
and actions involving a particular external destination.

| Information | Proposed home |
| --- | --- |
| Where the product is intended to be delivered and why | Documents, compiled into ordinary entities and requirements |
| What must survive delivery | Requirements and criteria |
| Allowed downtime, compatibility windows, recovery policy | Requirements and criteria |
| External object identifiers and ownership mappings | Destination binding |
| Last confirmed specification and artifact set | Immutable checkpoint references |
| Current provider response, revision, or migration progress | Observation evidence |
| Prepared actions and their dependencies | Transition plan |
| Started, completed, or uncertain external actions | Attempt record |
| Missing policy or unsatisfied obligation | Diagnostic, with the existing answer or ratification mechanism where needed |

Delivery instructions can use an entity's free-form stereotype, requirements,
relationships, criteria, and flow views. No new authored graph kind is needed to
state that a manuscript is published in a document or that a service preserves
orders during an update.

A destination binding explicitly selects its graph entities and scope. Scheduling
must not depend on guessing that every entity named Production is a destination.
The way that binding is created remains an implementation decision.

Operational evidence has an origin, time, scope, and relevant external revision.
It does not become a quoted requirement or a human decree. An observation that
suggests a policy change can support a diagnostic and a prose proposal. It must
not silently alter the desired specification.

## Destination

A destination is a stable identity for an independently addressed delivery target.
Examples include a public website, a customer installation, an editorial document,
or a publication channel.

Its binding identifies:

- The graph scope it serves, including relevant external dependencies.
- The actual external identity, such as a document id or bucket and prefix.
- Which objects or regions Jazyk manages.
- How to inspect it and what execution capabilities are available.
- Its confirmed checkpoint and any active attempt.

The destination identity must survive ordinary renames. Rebinding an editorial
copy to another document is a destination change, not a text edit. Planning must
establish whether the new object is empty, already managed, or needs adoption.

The specification can state non-secret addresses and provider choices when they
are product facts. Credential material stays outside graph and checkpoint content.
The split between prose, project settings, and runtime bindings needs to be
specified before implementation.

### Ownership and preservation

A destination needs a boundary around what Jazyk is allowed to change. The boundary
can include replacement, shared editing, preservation, and unmanaged regions. These
are responsibilities expressed by requirements and bindings, not a required enum
of all possible ownership policies.

E.g.:

```text
The website publisher manages generated files under the site prefix.
Uploads outside that prefix remain unchanged.
The manuscript body incorporates editorial changes.
Publication preserves the document's sharing settings.
Schema updates preserve customer records.
```

A missing object is different from an object the executor cannot read. An empty
response caused by missing access must not authorize replacement or initial setup.

The dependency scope can exceed the destination's owned objects. Old clients,
shared databases, queues, other publications, and links held by readers may impose
obligations even though Jazyk does not control them.

## Product candidate

A candidate identifies the particular product proposed for delivery. It contains
or references:

- A frozen relevant specification, including delivery policies and criteria.
- Exact artifacts or a manifest of immutable artifact references.
- Relevant source, configuration, and generated support procedures.
- Verification evidence tied to those artifacts and criteria.

The graph generation number alone is insufficient. Different builds from the same
graph can produce different bytes. Mutable source paths alone are insufficient for
the same reason.

Capture the candidate at preparation time. A later change to an artifact, procedure,
relevant requirement, or delivery policy invalidates the affected plan and its
evidence. An unrelated graph edit need not invalidate it when its dependency scope
is known and unchanged.

## Confirmed checkpoint

A checkpoint records a confirmed delivery. It is immutable. A destination's pointer
advances to another checkpoint only when the required verification succeeds.

Retain enough information to recover:

- The delivered candidate's relevant specification and criteria.
- The exact artifact set and any migration procedures still required.
- Bindings from product parts to external identities or regions.
- Evidence establishing what was delivered, where, and under which conditions.
- References needed by support or recovery obligations.

A checkpoint is evidence from a particular time, not a claim that the destination
still matches it. If a later attempt partially changes the destination, the old
checkpoint stays as the last confirmed baseline. The attempt and fresh observations
describe what has happened since.

The checkpoint may reference a composite delivery with component versions. Its
verification scope must be explicit. Confirming one component does not confirm
every destination that depends on it.

### What to snapshot

A full immutable graph snapshot is a reasonable first implementation for semantic
history. Sessions can load only the relevant closure. A later optimization may
store that closure alone if it preserves historical dependencies and provenance.

Historical quotes refer to retained historical source content. They must not be
checked against today's documents and reported as stale current requirements.
References to deleted or merged nodes remain meaningful within the frozen snapshot.

Artifacts need retained bytes or reliable immutable references. A digest detects a
difference but cannot reconstruct the old content. A Git revision is sufficient
only for the material it actually retains. It may not contain the built artifact.

The checkpoint need not hold all mutable business data. It may instead retain
schema identities, migration receipts, invariant-check results, and recovery
references. A shared document merge will usually require the actual previously
published content. The necessary evidence depends on the obligation.

### Illustrative envelope

E.g.:

```yaml
destination: editorial-copy
binding: binding-1
checkpoint:
  candidate: candidate-7
  specification: retained/specification-7
  artifacts: retained/manuscript-7
  mapping: retained/document-mapping-7
  evidence: retained/publication-check-7
active_attempt: null
```

These references illustrate separation and recoverability. They are not proposed
path conventions or the complete fields of a required YAML format. The document
id, revision, and section bindings live in the referenced destination records.

## Observation

An observation is evidence about a destination, collected for a particular purpose.
It identifies the tool or executor, target identity, time, relevant revisions,
observed scope, and unavailable information.

Possible contents include:

- Artifact digests or a published asset manifest.
- Database schema and migration progress.
- Supported client cohorts or evidence about active protocol versions.
- A document's text, revision, and section identities.
- Health checks, data invariant checks, and failed inspection attempts.

Do not impose one universal schema on all payloads. The common envelope establishes
identity, origin, freshness, and which plan conditions the evidence supports.
Destination-specific procedures interpret the payload.

Observations are partial. A report of zero active old clients does not prove that
an offline supported client can never return. A successful smoke test does not
prove all preservation obligations. Plans must expose what remains unknown.

Some state changes continuously. The plan should guard relevant revisions and
conditions, rather than hash the entire live database and invalidate on every new
order. The observation procedure defines the scope of each guard.

## Transition plan

A plan is a derived execution artifact for a selected candidate and destination.
It is not an authored semantic graph and it is not a list of board goals created
by the model.

Its common structure needs to establish:

- Input identities: candidate, baseline, destination binding, relevant policies,
  and observations.
- Preconditions and invariants that govern execution.
- Concrete actions, dependencies, and evidence needed before proceeding.
- Expected effects and the checks that establish completion.
- Conditions that permit waiting, retrying, resuming, or replanning.
- Recovery procedures and boundaries after which earlier recovery is no longer
  valid.

Detailed actions can be generated scripts, provider operations through existing
tools, external jobs, or steps performed by a person. The core does not need SQL
migration fields or Google Docs paragraph fields.

A core that accepts arbitrary prose claiming success would be too weak. Procedures
must expose checkable outcomes and action identities. A core that prescribes all
provider semantics would be too restrictive. The experiments must find a small
execution contract between those extremes.

Ordering comes from conditions in the particular transition. A plan can prepare
compatibility code, wait for an external event, migrate data, activate behavior,
and defer cleanup. It need not use every phase. A replacement of a simple file can
be one guarded action and a verification.

## Compatibility obligations

Compatibility is relative to a consumer, representation, or interaction under stated
conditions. It is not one property of a version.

Examples include a server accepting requests from an older client, old and new
processes sharing a database, a previous binary reading newly written records, and
a reader's chapter link continuing to resolve.

A support policy determines which historical commitments remain active. The planner
loads those commitments from retained checkpoints and current policy. Deleting a
current requirement alone does not prove that old consumers stopped relying on it.

Historical and current requirements have different scopes. Keeping them in frozen
contexts avoids presenting an intentional product change as an ordinary current
contradiction. A conflict between a proposed change and an active support policy
is a delivery finding that needs a compatible path or a policy decision.

## Lifetime and retention

Delivery records need a lifetime independent of the compiler's
[store reset rule](../../docs/compiler/graph.md#store-version). A rebuild from empty
must not turn an existing installation into an assumed empty destination.

Possible homes include an independent local operational store, a shared delivery
service, or records retained by existing providers with a local index. Selecting
one is an open design decision. The first implementation must prove recovery from
a removed compiler output directory before it is trusted with external changes.

Keep all records and artifacts referenced by:

- A destination's confirmed checkpoint.
- An active or unresolved attempt.
- A supported consumer or required upgrade path.
- A stated recovery policy.
- An explicitly requested historical record.

Garbage collection can remove unreferenced history. It cannot rely on regenerating
an exact artifact with an LLM. A retained reference whose backing content has
expired is missing evidence, not a valid recovery baseline.

If delivery memory is lost, inspect and adopt the destination with explicit
uncertainty. An inaccessible or unidentified destination does not become a fresh
installation. Reconstructing intent may require an owner decision even if the
external bytes are readable.
