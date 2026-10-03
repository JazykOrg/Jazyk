# Delivery worked examples

Status: proposal. These are hypothetical fixtures for testing the design, not
instructions to change Jazyk's production infrastructure. The procedures illustrate
the [common workflow](./workflow.md); provider-specific execution remains to be
implemented and validated.

## Static website in S3

### Stated obligations

E.g.:

```text
The public website is served from the production bucket through its configured CDN.
Publication preserves the public hostname and existing page links.
Supported browser sessions continue working during publication.
Generated assets are managed under the site's configured prefix.
Other objects in the bucket remain unchanged.
```

The docs also need a support policy for old pages and assets. An arbitrary retention
period invented by the publishing worker does not settle how long an old browser
session is supported.

### Baseline and observations

The checkpoint retains the delivered site artifact, its manifest, the relevant
requirements, and the destination binding. The observation reads the published
entry points, owned object identities, and relevant delivery configuration.

It checks whether an outside publication changed the site and whether any earlier
attempt left uploaded assets without activating them. It does not infer an empty
site from an unavailable bucket listing.

### Candidate transition

- Establish the complete candidate asset set and references.
- Upload new immutable assets before publishing entry points that refer to them.
- Check that the required assets are retrievable through the intended delivery path.
- Publish the new entry points using the destination's coordination mechanism.
- Apply the specified cache behavior and check the public result.
- Retain old assets while supported pages or recovery policy still need them.
- Confirm the new checkpoint with artifact and public verification evidence.

Cache invalidation does not establish that every browser has stopped using the old
page. Multi-object publication may expose mixed versions. Either the publication
mechanism controls that exposure or the candidate must work under it.

If the website uses an API, server compatibility belongs in the dependency scope
even when the website's owned objects are only static files.

### Recovery and evidence

A failure after asset upload but before activation should be recognized on retry.
It must not require deleting assets an old page still uses. Returning entry points
to a retained compatible candidate may provide recovery, subject to any API changes.

The fixture should verify old and new page loads, lazy asset requests from an old
page after activation, interrupted publication, unrelated object preservation, and
an outside change between preparation and execution.

## Stateful application with connected clients

### Stated obligations

E.g.:

```text
Each customer has a display name.
Existing customer names survive updates.
Protocol 1 clients remain supported until the stated retirement date.
Orders accepted during an update are retained.
An active client reconnects without losing an acknowledged order.
```

Suppose the existing database stores the name in `full_name`, the candidate uses
`display_name`, and old clients submit `fullName`. The candidate may pass every
fresh-install test while failing over the existing destination.

The planner must inspect implementation and bindings. An entity renamed in prose
does not necessarily rename the physical column or public field. If the mapping is
unchanged, that rename alone needs no migration.

### Baseline and observations

Retain the actual previous application, relevant client contracts, schema and
migration identities, and preservation criteria. Inspect the current schema,
running application versions, migration progress, relevant client evidence, and
the availability of the planned recovery procedure.

An offline supported client can return later. Observing no active protocol 1
connections is insufficient to retire protocol 1 before its policy permits it.

### Candidate transition

A possible transition for the physical column change is:

- Add the new representation while retaining the old one.
- Generate and deploy compatibility behavior that works with the actual mixtures
  of schema, server, and client versions allowed during rollout.
- Backfill existing values using a resumable procedure that handles concurrent
  writes explicitly.
- Establish that the new representation contains the required values and that
  every active writer maintains the representation the next step relies on.
- Activate the new read and write behavior under those conditions.
- Continue accepting the old protocol while its support obligation remains.
- Remove the old representation only after no supported reader, writer, or recovery
  procedure needs it.

This is a planning outline, not a complete migration algorithm. The generated
procedure must choose and test how concurrent writes remain consistent. A dual
write by new servers alone does not account for old servers that still write only
the old column. Triggers, staged writer changes, reconciliation passes, or a stated
pause policy have different consequences and require concrete evidence.

The temporary compatibility code is part of the candidate or an intermediate
candidate. Delivery analysis therefore opens ordinary product work before the
transition can run. The deployment executor cannot compensate for its absence by
changing command order.

### Connected sessions

Protocol compatibility and connection continuity are related but distinct. A new
server may understand an old request while terminating the connection that carries
it. The procedure needs the stated policy for draining, reconnecting, retrying,
and recognizing acknowledged work.

The core need not model every connection. It needs evidence for the relevant
conditions and tests for the externally visible contract. Long waits can be owned
by the external executor without keeping an LLM session open.

### Recovery and evidence

Recovery to the earlier application is valid only if it can use the schema and
values that now exist. A tested recovery before activation may become invalid after
new writes or cleanup. The plan records that boundary.

The fixture should exercise existing customers, orders created during migration,
old clients against new servers, supported mixed server versions, interrupted
backfill, lost acknowledgements, reconnect behavior, and recovery after new writes.
Checks compare preserved values and acknowledged effects, not just successful
process startup.

## Novel in a collaborative Google Doc

### Stated obligations

E.g.:

```text
The editorial manuscript is published into the existing Google Doc.
Publication preserves the document's identity and sharing settings.
The manuscript includes accepted editorial changes.
Editor comments remain associated with their intended passages.
Conflicting manuscript and editorial changes are presented for resolution.
```

These obligations require a capable publishing procedure. They do not establish
that replacing the entire document body preserves comments or that an automatic
text merge preserves editorial meaning.

### Baseline and observations

The checkpoint keeps the manuscript as last published, the specification that
produced it, and the mapping from stable product parts to document regions. The
observation reads the current document content and revision plus the evidence
needed for the preservation requirements.

The comparison has three versions:

```text
                 Last published manuscript
                     /              \
                    v                v
          New generated manuscript   Current document
```

A revision token is useful for concurrency control. Retained baseline text supplies
the content needed to understand intervening edits. One does not replace the other.

### Candidate transition

- Compare the new manuscript with the published baseline to identify intended
  product changes.
- Compare the current document with the same baseline to identify outside edits.
- Combine independent changes under the stated editorial policy.
- Present overlapping or semantically conflicting changes for resolution.
- Prepare edits against stable regions and the current document revision.
- Apply with a provider revision guard where available. If the revision changed,
  refresh the observation and reconsider the affected edits.
- Verify the resulting manuscript, preserved identities, and preservation criteria
  before confirming a checkpoint.

The [Google Docs write control documentation](https://developers.google.com/workspace/docs/api/how-tos/best-practices)
describes rejecting writes whose required revision is no longer current. That
addresses one concurrency race. It does not certify semantic conflict resolution
or comment preservation for every edit operation.

If available APIs cannot establish or preserve a required property, the plan must
say so. A human-assisted publication, a changed editorial workflow, or a revised
requirement may be necessary. Success cannot be inferred from text equality alone.

### Editorial changes and upstream work

Keeping an editor's change in the destination is separate from incorporating its
intent into future generation. Otherwise the next generated candidate may propose
removing it again.

Changes that alter the product's intended content need a path through the existing
proposal and answer mechanisms. The related
[upstream reconciliation proposal](../upstream-reconciliation.md) discusses that
loop for local deliverable edits. Delivery needs the corresponding handling of
external evidence and ownership mappings. Neither proposal should silently treat
all observed edits as new authoritative requirements.

### Recovery and evidence

A request can succeed while its response is lost. Before retrying, inspect whether
the intended edit already landed. Repeating an insertion can duplicate a chapter.
Restoring an earlier manuscript can also erase later editorial work, so recovery
needs fresh observations and the same preservation policy.

The fixture should cover independent edits, overlapping edits, chapter reordering,
comment anchors, identity and sharing preservation, a revision race, and a lost
write response. Use a controllable document fixture first, then establish which
properties can be checked against the real provider.

## Other media and destinations

The same reasoning extends to a handbook with public links, an updated course with
learner progress, or a deck published at a stable shared address. The relevant
preservation obligations differ; the four planning inputs remain the same.

Some publications cannot be updated in place. A printed edition or a distributed
download may require issuing a new edition, retaining access to the old one, or
publishing corrections. Delivery instructions must state those effects. The generic
model must not imply that every delivered product can be recalled or rolled back.

A single product can have several destinations with different baselines. The
editorial copy and a public edition may advance independently. There is no universal
global current release that describes both.
