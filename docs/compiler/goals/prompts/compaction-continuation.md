Continue the active session `{SESSION}`.

The preceding summary is historical context. The request that produced it was
runtime compaction, not a new user task. Any claim that the latest user request
is to summarize this history or stop the pending work belongs to that compaction
operation. It does not replace the actual user task or accepted steering.

Resume the actual pending work described in the summary under the current agent
contract. Use the available tools when actions remain. Retain completed tool
outcomes and goal gates. Do not repeat actions that already completed.
