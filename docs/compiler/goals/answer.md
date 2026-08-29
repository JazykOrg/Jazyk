# The answer goal

`answer` is where a question the compiler cannot settle waits for a person. A
diagnostic that carries a [prompt](../model/diagnostic.md#prompts) (one question, up
to four options, freeform allowed or not) is a standing question about the documents:
which of two bounds holds, whether a lookalike is one concept, whether a generated
choice is what the owner meant. The goal stands while the question is open and
unanswered, and nothing but a human closes it. No session runs it. It keeps the report
honest: a build with open questions is `converged, 2 blocked`, never silently done. See [answers](../model/diagnostic.md#answers).

- Class: compile. Blocked on a human: it never blocks convergence and rides in the
  verdict as a count.
- Unit: one diagnostic. Goal id `g:answer:diag:<rule>-<n>`.
- Skill: none. No contract paragraph: `prompts/` carries no `answer.md`.

## Created when

The goal derives from a `prompt-unanswered` [change record](../graph.md#change-records),
written by the commit that files a prompt on a diagnostic. `via` names what filed it:
the tool (`report_diagnostic`, `update_diagnostic`, `record_generation`) or the check.
The writers:

- Judgment sessions. [`rejudge-pair`](./rejudge-pair.md) attaches a prompt to a
  `contradiction` whose repair is enumerable: one `edit` option per side, rewriting
  the other document's sentence to agree. [`review-entity`](./review-entity.md) and
  [`dedupe-candidates`](./dedupe-candidates.md) attach one to an `ambiguity` or a
  `duplicate-entity` the model cannot settle from the quotes. A session that must
  hand a choice to the owner files a `decision` diagnostic whose prompt carries the
  question and the options ([`abstract-entity`](./abstract-entity.md) proposing
  component structure where the documents state none).
- Checks, mechanically, where the resolution is enumerable: `pinned-fact-drift` (the
  docs are right and the code must change, the value is stale here, or a freeform
  reply) and `unstable-derivation`, filed by
  [flip detection](../reconciler.md#flip-detection) with both justifications side by
  side (keep the split, keep the merge, or a freeform ruling). See
  [checks](../compilation.md#checks).
- Generation. `record_generation` files one `invented-choice` diagnostic per choice
  the deliverable needed and the documents never stated, graded by the scope of the
  invention; its prompt's `edit` option proposes the sentence the docs should gain
  ([invented choices](./generate.md#invented-choices)).
- Chat. An agent in a [chat session](../../frontends/acp.md#questions-in-chat) files
  or sharpens a question with `report_diagnostic` and `update_diagnostic`; the question
  it sharpens in chat is the same one the LSP shows inline in the file.

E.g.:

```yaml
- id: c415-2
  generation: 415
  mutation: 2
  kind: prompt-unanswered
  subject: diag:contradiction-3
  via: report_diagnostic
  detail: {rule: contradiction, subjects: [req:orders-6, req:payment-9],
           options: 3, freeform: true}
```

What derives no goal of this kind:

- A `ratification-pending` prompt. A proposal about the prose behind a derived or
  decreed fact is the [`ratify`](./ratify.md) goal; the two human seams stay distinct
  and both count as `blocked`.
- A diagnostic that already carries an `answer`. A re-detected condition on a node
  that carries one is not re-asked, so no record is written; a rejected suggestion
  stays rejected across rebuilds.
- A prompt on a diagnostic whose `triage` is `suppressed` or `wontfix`: the owner
  declined the question (below).
- A diagnostic without a prompt. A finding with no enumerable resolution is triage
  work in the [diagnostics queue](../model/diagnostic.md#lifecycle-and-triage), not a
  goal; the board carries questions, not every warning.

One goal per diagnostic. A session that finds the same condition again on subjects
already carrying an open prompt does not file a second: `report_diagnostic` on the same
rule and subjects resolves to the existing node, and `update_diagnostic` rewrites its
prompt in place. A rewritten prompt on an unanswered diagnostic refreshes the goal's
`change` without opening a second goal.

The goal exists exactly while the diagnostic is `open`, carries a prompt, has no
answer, and is not declined. The condition lapsing clears it as much as an answer
does: a later session resolves the diagnostic, the check that filed it finds it clear,
or its subjects die and the store settles it (`settle-diagnostics`). Nothing records
an answer in that case, so a condition that returns asks again.

## Gate

A human answer lands on the diagnostic: an option chosen, or freeform text
([answers](../model/diagnostic.md#answers)). The `answer` journal entry
([journal](../graph.md#journal)) records the goal under `resolved_goals` with the
chosen option's label, or the first line of the freeform reply, as its justification;
the change record clears; the goal is gone. Nothing a session does closes it:
`mark_goal_done` is never accepted for this kind, because no batch ever carries it, and
`answer_diagnostic` is a chat tool absent from every goal toolset.

Applying the answer is not part of the gate. An `edit` option applies in the same entry
(a dual write); an `answer` option or a freeform reply is applied by an answer session
that follows (below). A session that fails leaves `answer.status: failed` on the node
with the error; the goal does not derive again, because the compiler never re-asks a
question a person already answered. The failure surfaces where questions surface
([on status surfaces](#on-status-surfaces)), and running the reply again is a new
answer session over the same recorded text.

Declining is the other way out. Setting `triage` to `suppressed` or `wontfix` on the
prompted diagnostic journals a `triage` entry that clears the record: the prompt stays
on the node unanswered, the finding stands, and the compiler never asks again.
`acknowledged` says seen, not declined: the goal stands.

## Hints

Rendered for the human, on the goal card and in `jazyk explain`:

- The question, each option with its label, and whether a freeform reply is accepted.
  For an `edit` option: the target document and section and the replacement text.
- The subjects with their statements and verbatim quotes, and the filing session's
  `reasoning`. For `unstable-derivation`: the two justifications side by side.
- The cause: which commit filed the prompt, and the goal that commit was resolving.
- Where to answer: the file (an LSP code action at the subject's quote), the GUI
  questions panel, or chat.

## What the model sees

No session claims an `answer` goal, and no contract paragraph exists for it. A session
sees the diagnostic like any open diagnostic on a node it loads: the
[loaded set](../context.md#the-loaded-set) lists open diagnostics on a loaded node with
their rule, severity, and message, and marks one that awaits an answer. The
`## Project` block counts the goals blocked on human answers, so a session knows the
question exists and is not its to answer. A session never answers a prompt, and it
never files a second one for the same condition: it may sharpen the question with
`update_diagnostic`, or resolve the diagnostic when the quotes show the condition has
lapsed.

## The human path

Answering is a human act through any frontend, and every path lands the same `answer`
journal entry:

- The LSP shows the diagnostic at its subject's quote and offers a code action per
  option: `Apply:` for an `edit`, `Answer:` for an `answer`
  ([capabilities](../../frontends/lsp.md#capabilities)).
- The GUI's questions panel lists every open prompt with its options and a freeform
  field ([questions](../../frontends/gui.md#questions)); the board card links there.
- A chat session sends one summary of the open questions on start and lists them again
  on `/questions`; a person answers in plain chat, and the session's agent records it
  with `answer_diagnostic` ([questions in chat](../../frontends/acp.md#questions-in-chat)).

### Apply an edit

Choosing an `edit` option is deterministic: no model runs. The serving applies it as a
dual write ([edit paths](../compilation.md#edit-paths)):

- The replacement lands in the document.
- The section hashes are absorbed in the same changeset, so the edit does not dirty
  the document it just changed.
- A requirement whose own quoted sentence the edit rewrites is re-anchored in the same
  changeset, its `statement` mechanically updated when the replaced text appears in it
  verbatim. Anchors that cannot be re-anchored mechanically go stale, and
  [`reconcile-section`](./reconcile-section.md) addresses them next build.
- The diagnostic resolves with the option's label as the reason
  (`answer.status: applied`); the change record clears; the goal is gone.

Downstream goals derive from the graph change as usual: a revised requirement opens
[`rejudge-pair`](./rejudge-pair.md) on its neighbors and [`bind`](./bind.md) on its
row. An `invented-choice` accepted this way writes the choice into the documents, and
the next build extracts it as a requirement, so the invention becomes specified.

### Reply

Choosing an `answer` option or replying freeform records the text on the node with
`answer.status: handling` and resolves the goal. A model then acts on it:

- In a chat session, the session's own agent acts on the reply with the chat serving's
  tools, then resolves the diagnostic (`resolve_diagnostic`) or refines its question
  and leaves it open (`update_diagnostic`).
- Anywhere else (an LSP code action, the GUI panel), jazyk spawns an
  [answer session](../../frontends/acp.md#answer-sessions): the same shape as a worker
  session, the `chat` serving injected, the prompt carrying the diagnostic, the
  question, the reply, and the subjects in the loaded set, with the contract to act on
  the reply and then resolve or re-prompt. It is not a goal batch: it runs between
  build sessions under the store lock ([concurrency](../graph.md#concurrency)), carries
  no goal tools, and the recorded answer is its cause.
- The session's changeset journals as an `answer` entry naming the diagnostic, with the
  mutations it staged; every goal those mutations open carries that generation as
  its cause, so [`jazyk ripple`](../../frontends/cli.md#jazyk-ripple) roots the cascade
  at the human's reply.
- `answer.status` moves to `handled` when the session lands, or `failed` with the
  error after one retry. Every frontend shows the same progress from the store.
- A re-prompt is a new question: the refined prompt writes a fresh
  `prompt-unanswered` record, and a new `answer` goal derives for it.

Two replies have consequences the harness owns:

- `unstable-derivation`: the ruling clears the parked pair, and the next build resumes
  the chosen direction ([flip detection](../reconciler.md#flip-detection)).
- `pinned-fact-drift`, when the reply says the docs are right: the diagnostic resolves
  with the ruling, and the requirement's row is the deliverable's debt; the bound test
  encodes the pinned value, and the row's verdict says whether the code caught up
  ([the cascade](../../consumers/gen.md#the-cascade)).

## On status surfaces

- The verdict counts it: `converged, 2 blocked` or
  `incomplete: ... 2 blocked ...` ([convergence](../compilation.md#convergence)).
  Blocked goals never block convergence; they ride as counts.
- `jazyk compile` prints `N blocked` in its board summary line; the session prompt's
  `## Project` block counts goals blocked on human answers; `jazyk status` shows the
  board counts; `jazyk watch` prints one line when the goal opens and one when it
  resolves; `jazyk explain g:answer:diag:<rule>-<n>` prints which commit filed the
  prompt, the question with its options, and where to answer
  ([CLI](../../frontends/cli.md#jazyk-explain)).
- The GUI board shows a card in the compile column, blocked with the reason
  `awaiting answer`, linking to the questions panel; the inspector shows the prompt on
  each subject ([GUI](../../frontends/gui.md)). A `failed` handling shows on the same
  card and in the panel, with the error and a way to run the reply again.
- The LSP shows the diagnostic inline with its code actions; hover on a subject names
  the open question.
- Chat lists it in the session-start summary and under `/questions`.
- [Docsgen](../../consumers/docsgen.md#the-requirements-document) lists it under the
  subject entity's open diagnostics with the question.
- `goals({})` over MCP lists it as `blocked` with the reason; the empty-board `goals`
  reply and the final `done` reply carry the open diagnostic counts beside the
  verdict ([compilation over MCP](../../frontends/mcp.md#compilation-over-mcp)).

## Tools

No session tools. The human's tools: `answer_diagnostic({id, option?, text?})` in chat
([chat tools](../tools.md#chat-tools)), the LSP code action, the GUI questions panel,
and the triage controls that decline a question. Every answer lands the same `answer`
entry; every decline lands a `triage` entry.

The tools that create the goal belong to sessions and workers:
`report_diagnostic({rule, severity, subjects, message, reasoning, prompt?})` and
`update_diagnostic({id, prompt})` in the judgment toolsets
([write tools](../tools.md#write-tools)), `record_generation`'s invented choices
([generation tools](../tools.md#generation-tools)), and the checks that attach prompts
mechanically.
