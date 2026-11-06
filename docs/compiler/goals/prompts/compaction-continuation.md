Continue the active session `{SESSION}`.

The preceding summary is historical context. The request that produced it was
runtime compaction, not a new user task. Any claim that the latest user request
is to summarize this history or stop the pending work belongs to that compaction
operation. It does not replace the actual user task or accepted steering.

Resume the actual pending work described in the summary under the current agent
contract. Use the available tools when actions remain. Retain completed tool
outcomes and goal gates. Do not repeat actions that already completed.

The retained package and tool exchanges below carry exact tool evidence. Read them
before reloading packages or files merely because the summary omits their values.
The latest package remains a historical snapshot; a changed graph or source scope
can still require a fresh package.

Before a pending recording call, check that its authoritative package is available
with exact identifiers, file roots, hash and existing bindings. If the summary
omits or guesses any of them, reload begin_generation or begin_binding for the
current target. Reuse existing binding objects exactly; a suggested test name is
not an existing selector. A reload does not require repeating completed writes
or starting another test run.
