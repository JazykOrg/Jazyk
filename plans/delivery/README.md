# Delivery design proposal

The [Cloud and delivery plan](../cloud/README.md), dated 2026-10-03, integrates this
design with named TOML deliveries, Git publishing, script entrypoints, GitHub Actions,
and PostgreSQL-backed cloud operation. Use its sequence for implementation. This
directory retains the destination, checkpoint, preservation, and recovery reasoning.

Status: proposal, recorded 2026-10-03 from the delivery design discussion. The owner
requested options for a generic delivery concept and asked to retain the reasoning.
The recommendation below is a proposed direction. Recording it does not make it
accepted compiler behavior. Accepted behavior must enter `docs/` before implementation.

## The problem

Jazyk generates a product. That product reaches a destination. A later product must
be delivered into a world that already contains the earlier one and has continued
changing since delivery.

That world may contain database records, active connections, cached browser code,
editorial changes, comments, bookmarks, or links other people keep. Replacing the
generated files does not describe what should happen to any of those things.

The owner wants delivery instructions that work for software and other media. An
S3 website and a novel published into a Google Doc should fit the same concept.
The owner does not want to manage every deployment or maintain a release catalog
unless the product actually needs one.

## Recommendation

Add an on-demand delivery consumer centered on destinations. Keep a confirmed
delivery checkpoint for each destination. Observe the destination again when
preparing an update. Derive the transition from the checkpoint, the new product,
the observation, and the delivery obligations stated in prose.

```text
Last confirmed delivery ---+
New product and spec ------+--> Transition plan --> Execute --> Verify
Destination observed now --+                                  |
Delivery obligations ------+                                  v
                                                   Confirmed checkpoint
```

The consumer needs enough memory to plan the next change and recover an interrupted
one. A complete history of successful attempts is optional. Older checkpoints stay
while a supported consumer, destination, recovery policy, or unfinished transition
still needs them.

The instructions remain natural language requirements. The operational records
identify artifacts, external objects, observations, and progress. Destination
details are supplied by generated procedures and existing tools.

## Reading order

- [Options and reasoning](./options.md): three approaches, execution choices, and
  refinements to the earlier deployment proposal.
- [Representation](./model.md): the graph boundary, destinations, checkpoints,
  observations, transition plans, storage, and retention.
- [Delivery workflow](./workflow.md): preparation, execution, partial completion,
  recovery, compatibility, and feedback into generation.
- [Worked examples](./examples.md): an S3 website, a database and connected clients,
  and a collaboratively edited Google Doc.
- [Implementation plan](./implementation.md): staged experiments, docs-first
  integration, acceptance criteria, and decisions still to make.

## What exists today

- The [generation ledger](../../docs/consumers/gen.md#the-ledger) records generation,
  bindings, artifacts' file references, and verification state. It is mutable
  metadata for the current deliverable.
- The [deliverable baseline](../../docs/consumers/gen.md#incremental-regeneration)
  keeps files as they stood before the last generation run changed them. It serves
  a local diff. It does not identify a confirmed external delivery.
- A [control-plane release](../../docs/compiler/control-plane.md#modes-and-releases)
  grants permission to compile, generate, or decompile. Its meaning is distinct
  from publishing a product.
- The [journal](../../docs/compiler/graph.md#journal) records graph changes and
  causes. Its generations are not product releases or proof of external effects.
- The [graph](../../docs/compiler/model.md) already supports entities, requirements,
  criteria, relationships, and flows across media. Delivery instructions can use
  those concepts.
- The [store version rule](../../docs/compiler/graph.md#store-version) archives the
  output directory and starts empty on a version mismatch. Delivery memory needs
  a separate lifetime or a reliable external recovery source.

The earlier [product and deployment proposal](../product-and-deployment.md) explores
artifact presentation, product versions, migrations, and deployment. This proposal
develops the delivery problem further. It does not change that file's implementation
status or adopt its remaining phases. The differences are recorded in
[options and reasoning](./options.md#relationship-to-the-earlier-proposal).

## The central distinction

A finished product can meet its current specification while being unsafe to deliver
over the product already in use. Some obligations constrain the end state. Others
constrain what happens during the change.

E.g.:

```text
The orders table contains a currency column.
Every existing order retains its total and currency during an update.
Supported older clients can submit an order while the server is being updated.
```

These are separate requirements. The first can pass on an empty new database while
the other two fail in production. Delivery requires evidence about the transition.

A transition can reveal missing product behavior. Compatibility code, a backfill
tool, a temporary reader, or a document merge procedure may have to be generated
before delivery becomes possible. Delivery analysis therefore feeds work back into
the existing design, binding, generation, and verification process.

## Vocabulary

| Term | Proposed meaning |
| --- | --- |
| Product candidate | A specific specification and artifact set selected for delivery. |
| Destination | An independently addressed place or publication that receives a product. |
| Checkpoint | Retained evidence of a confirmed delivery, including recoverable specification and artifact references. |
| Observation | Evidence of what exists at the destination now, with scope and uncertainty. |
| Transition plan | Actions and checks that carry an observed destination toward the candidate under the stated obligations. |
| Attempt | One execution of a particular plan, including partial progress and uncertain outcomes. |
| Compatibility obligation | A commitment to a supported consumer, stored format, identity, or interaction that the transition must preserve. |

These are planning terms. They do not commit Jazyk to new graph node kinds, a file
schema, command names, or a provider registry. A destination may use familiar product
words such as production, editorial copy, or publication.

## Design boundaries

- Delivery runs on request initially. A document save does not start external work.
- Destination purpose and preservation rules are ordinary prose requirements.
- Operational evidence stays outside the authored semantic graph unless it gives
  rise to an explicit proposed requirement or diagnostic.
- The harness owns work derivation, identity, readiness, validation, and execution
  bookkeeping. The model reasons about meaning and proposes how to satisfy the
  supplied work. It never creates or prioritizes board goals.
- Tool and provider capabilities bound what can be promised. A preservation
  requirement needs a realizable procedure and evidence.
- An external write is not part of an atomic graph changeset. Partial completion
  and uncertain outcomes are normal states the design must handle.
- Current product verification, transition readiness, and confirmed delivery stay
  separately visible.
- Compatibility has a stated scope and retirement policy. It is not an obligation
  to support every historical behavior forever.
- Credentials remain in the executor's existing credential mechanism. Operational
  records hold references and identities, not secret values.

## Research used in the discussion

These sources supply examples of existing mechanisms. The proposed Jazyk design is
our inference from the problem and Jazyk's architecture.

- Terraform refreshes provider observations before planning changes. This supports
  separating remembered state from current evidence.
  [State refresh](https://developer.hashicorp.com/terraform/tutorials/state/refresh).
- Google Docs supports revision controls for concurrent edits. This provides a
  concrete example of a destination-specific guard.
  [Collaboration and write control](https://developers.google.com/workspace/docs/api/how-tos/best-practices).
- AWS describes preparing readers before activating a new written format and
  testing coexistence and rollback. This illustrates why transition correctness
  differs from correctness of either endpoint.
  [Rollback safety](https://builder.aws.com/content/3F04j2yRAAMBuPSPs50xwXZqg01/ensuring-rollback-safety-during-deployments).
