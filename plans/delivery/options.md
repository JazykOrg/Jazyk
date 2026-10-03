# Delivery options and reasoning

Status: proposal. Read the [overview](./README.md) first. These alternatives describe
how much operational responsibility Jazyk takes. Execution can be delegated under
any of them.

## Option 1 Generate delivery procedures

The documents describe destinations and delivery obligations. Generation produces
scripts, migration tools, publishing procedures, workflows, and their tests. An
external agent or deployment system inspects the destination and runs the update.

State lives in existing systems: a deployment service, Git, migration records, an
artifact registry, document revisions, or the procedures' own records. Jazyk may
consume those records when asked to analyze another delivery.

### What this buys

- A small addition to the current generation consumer.
- Immediate use of established tooling and existing CI pipelines.
- No universal deployment schema or mandatory release management interface.
- Different media can use different procedures without changing the compiler.

### What it leaves outside Jazyk

- Reliable identification of what reached each destination.
- Preservation of the baseline needed for a later comparison or merge.
- Coordination and recovery after a partially completed update.
- Evidence that external execution matches the candidate Jazyk verified.

An agent can inspect a live system, but inspection does not always recover its
history. A current document does not reveal which paragraphs were generated and
which were edited later. A database schema does not establish the intended meaning
of a transformation. A version string does not recover the artifact bytes.

This option works when external systems already retain enough evidence and expose
it clearly. It is also a useful first execution mechanism for option 2. It becomes
unreliable if each delivery starts with an agent guessing what happened last time.

## Option 2 Keep a checkpoint per destination

Jazyk retains the last confirmed delivery at each destination, plus evidence needed
for active support and recovery. It inspects the destination when an update is
requested and derives a transition from four inputs:

- The confirmed checkpoint.
- The selected candidate and its relevant specification.
- Current observations.
- Delivery and compatibility obligations.

The checkpoint identifies a recoverable specification and the actual artifacts,
external identities, ownership mappings, and verification evidence. An attempt
records progress while external work is in flight. Completed attempt history is
optional once its evidence is no longer needed.

### What this buys

- A direct answer to what should change at a particular destination.
- An explicit baseline for changes made by other people or tools.
- Upgrade and recovery reasoning tied to actual artifacts and conditions.
- A small default interface: a destination, its confirmed state, and its pending
  update.
- The same structure for software, documents, and other generated products.

### What it costs

- Durable operational storage and artifact retention.
- Observation and verification procedures for each supported destination.
- Execution bookkeeping that survives interrupted and ambiguous writes.
- A deliberate boundary between semantic requirements and external evidence.

This is the recommended starting architecture. Its unit is an independently
updated destination. A project can have one such destination and no named releases.
Additional history becomes useful only when consumers and recovery policies need it.

One checkpoint does not imply that the destination contains exactly one version.
A rolling update, a client population, or a partially completed migration can
contain several versions. The observation and active attempt must represent that
mixture. A checkpoint must not claim a uniform final state prematurely.

## Option 3 Model versions and upgrade paths explicitly

Jazyk retains product versions and supported transitions as durable concepts. A
destination refers to its installed version or component versions. Upgrade planning
selects a supported path through historical versions and migration procedures.

Versions need immutable specifications and artifacts. Compatibility relationships
state which readers, writers, clients, and formats can coexist. Retirement policy
determines when a historical version and its transitions can be removed.

### What this buys

- Planning across many independently updated customer installations.
- Explicit support for installations that skip releases.
- Queries about supported upgrade paths and required intermediate versions.
- A durable explanation of support and compatibility promises over time.

### What it costs

- Temporal scope for requirements and judgments.
- More retention and version identity rules.
- A broader user interface and operational catalog.
- Potential pressure to turn the semantic graph into an operational inventory.

This can become necessary for self-hosted business software or long-lived device
fleets. It should be driven by those requirements. A manuscript and a small website
should not have to adopt it. The checkpoint approach can retain multiple supported
baselines before a fully versioned graph becomes useful.

## Comparison

| Question | Procedures | Destination checkpoints | Versions and paths |
| --- | --- | --- | --- |
| Who remembers delivery? | External tools | Jazyk or its operational store | A version and installation catalog |
| Default planning input | Live inspection and external evidence | Checkpoint, candidate, observation, obligations | Supported historical path plus current observations |
| Recovery owner | External executor | Plan and attempt protocol, possibly delegated | Transition catalog and attempt protocol |
| Suitable first use | Existing CI or publishing workflow | Website, application, collaborative document | Many independently updated installations |
| Main risk | Missing or ambiguous baseline | Treating the checkpoint as current truth | Adding version machinery before it is needed |

## Execution is a separate choice

The plan and checkpoint model should survive a change of executor. Possible
executors include a Jazyk worker, an attached agent, CI, a provider's deployment
service, or a human carrying out a documented step.

The first implementation should generate or reuse executable procedures and
support external execution with evidence import. A pipeline can own long waits,
infrastructure changes, and its own credentials. Jazyk retains the mapping between
the requested transition and its result.

Direct execution is an incremental capability. It needs a bounded contract for
preconditions, action identity, completion evidence, and recovery. A zero exit code
alone cannot confirm that the intended product is visible at the destination.

Likewise, continuous reconciliation is an independent future choice. The proposed
initial consumer runs on request. Scheduled observation or automatic application
would require stated policy and should not follow implicitly from ordinary builds.

## Why snapshots alone are insufficient

A specification snapshot answers what was intended. An artifact snapshot answers
what bytes were produced. A confirmed checkpoint answers what was verified at a
destination at a particular point. Fresh observations answer what is known now.

Each solves a different part of the problem. None replaces the others.

Snapshots of all production data are neither necessary for every delivery nor
sufficient for recovery. Restoring a database backup may discard writes accepted
after the backup. A code snapshot says nothing about whether a still-open browser
can use the newly deployed API.

The useful minimum is recoverable baseline content plus current evidence and
explicit preservation obligations.

## Why delivery analysis can change generation

A final schema may be correct while the generated application cannot reach it
without downtime or data loss. The transition may require temporary code that
supports both representations. No ordering of deployment commands can manufacture
that missing behavior.

Delivery analysis therefore needs to produce findings that enter the existing
requirements and generation loop. The model explains the missing behavior. The
harness derives the resulting work. Product policy choices follow the existing
ratification and answer paths.

The delivery plan is then regenerated against the changed candidate. It must not
silently apply the old plan to a newly generated artifact.

## Relationship to the earlier proposal

The [earlier product and deployment proposal](../product-and-deployment.md) contains
useful foundations: exact artifact identity, destinations, transition tests,
generated deployment procedures, and explicit recovery decisions.

This proposal refines several parts:

- Recording every green generation as a retained product version is optional.
  Capture a candidate when delivery or a support obligation needs it.
- Compare against current observations as well as the earlier delivery. A stored
  deployed-version field cannot establish that nothing changed outside Jazyk.
- Use a destination that can represent a document or publication as naturally as a
  running environment.
- Derive action order from the transition. Migrations do not universally precede
  an artifact switch. Some require compatibility code to run first.
- Treat recovery as a plan from the observed state. Reversing migration order and
  restoring old code is not a universal rollback algorithm.
- Retain exact artifacts while obligations need them. Regeneration from graph and
  journal is not a substitute for retaining exact delivered bytes.
- Inspect externally visible effects before treating a graph rename as a
  migration. A concept renamed internally may leave its stored representation and
  public contract unchanged.
- Keep runtime observations in operational records. A new model provenance kind
  is not required merely to record that a provider reported a revision.
- Separate lifetime of delivery evidence from the compiler's resettable output.

These are proposed refinements, not claims that the earlier plan has been
implemented or that this direction has been approved for implementation.
