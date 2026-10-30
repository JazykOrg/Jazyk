# Embedded agent runtime

The embedded agent uses the public Goose GDK state machine, inference, tool
execution and conversation compaction components. The same runtime serves native
CLI sessions, the bundled ACP agent, hosted jobs and browser sessions. External
executors remain external.

Jazyk owns scheduling, prompts, MCP tools, graph context, mutation gates, task
grants, budgets, persistence and completion. The runtime does not derive goals or
write around their gates. It does not import the Goose application facade. The former embedded loop, text
tool protocol and handwritten streaming collector are removed.

## Models and tools

Models must support native tool calling. A rejected native tool request produces
an explicit capability error. There is no text tool codec, toolshim or automatic
tool downgrade. Historical transcripts and benchmark results remain readable.

The native provider uses the configured LocalRouter endpoint and existing
configuration precedence. Hosted inference uses the host's admitted transport.
Browser inference uses its model host. Public GDK wire helpers preserve tool call,
reasoning and usage metadata. Provider credentials remain host capabilities.
Native HTTP requests use streaming transport. The maintained SSE decoder and GDK
collector produce a complete native response before its tools execute. Text and
reasoning deltas reach the owner thread while inference is pending. The final
message retains the complete response and does not repeat its streamed prefix.
The saved model receipt includes the prefix already delivered to the event stream.
Restoring that receipt does not repeat the prefix. Compaction summaries do not emit
user-facing deltas.

Tools come from MCP. A dynamic GDK tool provider delegates list and call operations
to the existing serving. Hosted and browser tools execute in process against their
virtual workspace. Native remote MCP uses the maintained Rust MCP SDK. Tools are
authorized again before execution and after recovery.

Hosted profiles register no native developer file tools, shell tools, automatic
extensions or subprocess transports. Image paths in model text do not authorize
host filesystem reads. File edits use the existing workspace mutation contract.

## Execution

The pipeline checks Stop, suspension, ownership and budgets before new work. It
delivers accepted input once, executes pending authorized tool calls, applies goal
completion gates, compacts when required and admits the next inference.

The operation pipeline runs on the workspace's owning thread. A tool must not move
to a thread that lacks its virtual filesystem scope. Missing or mismatched hosted
scope fails closed. Concurrent projects have separate scopes and state.

The session and host boundaries are asynchronous. Deterministic compiler work
remains synchronous. Native entry points may drive a session through a blocking
wrapper on its owner thread. Browser execution yields to JavaScript promises;
it must not block that worker while waiting for a promise on the same worker.

GDK events feed existing traces, chat updates and usage events. Hosts drain event
channels and bound retained events. A slow client cannot block the compiler or
create an unbounded queue.

## Compaction

Jazyk invokes GDK compaction explicitly at valid conversation boundaries. It keeps
the upstream compaction templates. The replacement history, summary, usage and
accounting identity form one execution transition. Pending tool calls retain their
paired results and required provider metadata.

Compaction uses the same admitted model route and job limits. Its cost counts
toward the job. Graph loaded-set policy remains independent of conversation
compaction. Jazyk retains its fixed agent contract and domain skill payloads.

## Checkpoints

An execution checkpoint is protected operational state. It is separate from graph
`status.yaml`, diagnostic exports and project files. Its format has its own version.

It records the logical job and command, conversation and accepted inputs, native
GDK state, operation identities, current batch and parent continuation, complete
model responses, tool intentions and receipts, staged MCP state, loaded handles,
goal outcomes, usage, remaining budgets, revision and event cursor.

Runtime locks, futures, credentials and thread-local scopes are not serialized.
Restoration reconstructs handles under a fresh valid lease and validates the
recorded workspace revision. The compiler resumes its active batch before choosing
new work. A completed batch receipt prevents duplicate parent execution.

Nested compilation retains the outer tool intention and each active inner session.
Hosted utility inference retains its response under a key derived from its prompt.
A parent retry reuses the response until it acknowledges that work.

Parent progress can be checkpointed between sessions without an active model
conversation. A running chat retains its accepted input and event cursor in the
same workspace checkpoint. Restoring it does not append the opening input again.

The capsule and workspace are one atomic host write. A local tool without a saved
receipt can restart against that saved pre-effect workspace. Remote task submission
or cancellation without a destination receipt pauses for reconciliation.

The host saves a model response before dispatching its tools. It saves each tool
intention before dispatch. A local mutation, result and MCP checkpoint commit
together. A repeated operation identity returns its recorded result. A changed
request digest under that identity is rejected.

Remote effects use destination idempotency or queryable receipts. An uncertain
outcome is reconciled before continuation. If it cannot be proven, execution
pauses explicitly. A missing response does not authorize blind replay.
Compiler task execution saves its destination operation key before submission.
It retains the returned run ID, original waiting deadline and collected result.
Deployment resumes waiting on that run. It does not submit another task or extend
the deadline. User Stop still cancels the owned run.

Checkpoint failure stops new work. Debug export pruning cannot remove active
recovery state. Checkpoints retain tenant protection and contain no provider keys.

## Deployment and Stop

Deployment suspension differs from user Stop. Draining closes new admission and
allows in-flight operations to reach a durable boundary. It then suspends the same
job and releases ownership. No terminal finished event is emitted for suspension.

A replacement worker validates compatibility, claims a new lease, reconciles
pending operations and continues automatically. Expired ownership follows the
same recovery path. A resumed execution releases only its matching saved control
claims before build admission. Restored MCP sessions reclaim their batch and ledger
claims under the replacement worker. A foreign claim still refuses admission.
Model dispatch, task submission and checkpoint writes validate
ownership and execution sequence. A stale owner cannot claim new work.

Stop is final. It wins over reclaim and does not auto-resume. Suspension does not
reset limits, extend task deadlines, create a new billed turn or redeliver input.
Partial model text is provisional and restored from stable message identities.
Recovery reconciles retained job chat events after the saved workspace cursor.
Their already published model prefix is added to the active request receipt, so
replaying a saved complete response does not append that prefix again.
Stopping a suspended job retains its already committed workspace changes. The
host publishes those saved bytes under a fresh ownership fence, closes its running
conversation and discards the staged MCP changeset without running the model.
Stop releases the stopped execution's batch, ledger and build claims. Earlier
commits remain. A lease is released only when its saved task, worker and claim
time and nonempty random `claimId` still match. A replacement claim gets a new
ID even when its worker and timestamp match. Legacy claims without an ID remain.
Claims belonging to another execution remain. Cancellation
discards an open changeset before returning, including after a checkpoint error.

The first upgrade lets legacy agents finish because they have no checkpoints.
Later incompatible upgrades or rollbacks wait for affected jobs to finish. Graph
store migration and graph archival are not execution recovery mechanisms.
