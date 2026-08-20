# Prompt review notes

Observations collected while decompiling the turn prompting into
`docs/compiler/turns/` (2026-08-20). Working notes for the owner, not design canon.
Delete freely once acted on or dismissed.

## Structural

- Two contract texts per deliverable task. `generate-entity` and `bind-requirement`
  each have a session-form contract (`prompts/generate-contract.md`,
  `prompts/bind-contract.md`, served in the `begin_*` reply) and a packaged-form
  system prompt (`prompts/generate-entity.md`, `prompts/bind-requirement.md`, used
  by the benchmark and `task_prompt`). They overlap heavily but are worded
  differently, and only one of them is what live runs see. The benchmark therefore
  grades a text production does not use. Worth collapsing to one text per task.
- Small templates still live in code only: the medium-decision system line and its
  JSON-contract user template (gen.rs `decide_medium`), the pipeline generation
  user prompts (product part, tests, manifest), the verify judge instructions
  (verify.rs `task()`), the MCP serving instructions (`mcp.rs instructions_for`),
  and the chat-serving text. Same treatment (extract + embed) is possible if wanted.
- The feedback note is inserted by splitting the system prompt on its first blank
  line (`with_feedback_note`). A prompt file that loses its blank line silently
  changes the insertion point. An explicit `{feedback}` slot would be sturdier. The
  splice guard now covers all five system prompts, so a payload edit that breaks the
  role-line-then-note shape fails the build; the brittleness itself remains.
- Payload files cannot carry comments: whatever is in the file reaches the model.
  Commentary lives in the per-task doc instead. If inline commentary is ever
  wanted, a comment-stripping load step (or a real template engine) is the price.

## Prompt content

- The reconcile system prompt is 11k characters of dense rules, accreted one
  counterexample at a time (addProduct, pronoun subjects, non-normative refusals).
  Local models must hold all of it while reading sections. Candidates: move the
  worked examples into a separate examples block, or split rules by concern and
  include only what the work item needs (a doc with no code fences does not need
  the pseudo-code rule).
- The review-entity prompt asks one diffuse question ("is this entity coherent?")
  across up to nine duties. The review-requirement prompt's own rationale says
  focused pairwise questions are what weak models answer reliably; the example-sort
  miss (a 27B model shown the conflicting pair in a 16-row list and not seeing it)
  confirms it. Candidates: enumerate deterministic suspect pairs in the pack (a
  concrete-value statement against the entity's unconditional rules), or split the
  entity review into narrower checks.
- ~~The verify judge parses the first `PASS`/`FAIL` occurrence anywhere in the
  reply.~~ Fixed: a first line that leads with the word is the verdict, otherwise
  the reply is read from its conclusion (`verify.rs parse_verdict`, unit-tested).
- The worker protocol line says "Do exactly this one task, then stop" after the
  system prompt already framed the turn; the pointer prompts repeat the same. Minor
  duplication, cheap tokens, but three texts state the stop rule differently.

## Costs

- Every turn resends its full system prompt; a build with dozens of turns pays the
  11k-character reconcile prompt each time. At local-model speeds this is minutes
  per build. Prompt-prefix caching (endpoint-side) or a shorter reconcile prompt
  would both help.

## Related design threads (already discussed, not prompt-local)

- Requirement-level dependency edges (`verifies`, `refines`) with sticky pair
  review, so changing a test re-judges it against what it tests deterministically
  instead of lexically. The pair-review pack would then carry the dep partners.
- The pair-review pack could include a capped one-line listing of all same-entity
  requirements, licensing the cross-pair findings its prompt already allows.
