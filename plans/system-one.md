# Plan: System One models

Status: exploration, not scheduled. Written 2026-09-24 as a study of where a
System One model fits jazyk. Nothing here is implemented or documented under
`docs/`. If any of it proceeds, the docs-first rule applies: a design page under
`docs/compiler/` and edits to the LSP, project settings, and benchmark docs land
before code.

## What a System One model is

The term is TypeSafe AI's, for their model Jev. A System One model takes text in
and returns typed decisions out: a choice from a fixed option list, a calibrated
score, or a boolean, each with a probability. It never emits a string. It cannot
write a statement, a definition, a quote, a justification, or a diagnostic
message. It has no tool loop and no reasoning trace.

Claims from their material, unmeasured by us:

- Latency 70 to 500 ms end to end, roughly two orders of magnitude under a
  frontier chat model on the same decision.
- Input priced at $0.042 per million tokens, output free.
- Option cardinality up to 255; quality degrades past roughly 100 options, so
  large candidate sets go through elimination rounds (tournament sampling).
- Text and JSON inputs only.
- Any chat model approximates one: constrained output to a single token, batched,
  with logprobs as the probability. Slower and uncalibrated, but portable.

The one constraint that shapes everything: the question is not which goal kinds
it can run, but which decisions inside jazyk are already enumerated. Jazyk's
division of labor makes that easy to answer. The harness already reduces most of
the model's job to a verdict with a gate. Every gate whose evidence is a verdict
plus an existing mutation is a System One shape. Every gate that needs prose is
not.

## Where the schema fits compilation

Several places in the compiler stand in for judgment with a weak deterministic
heuristic. Each is a scoring or choice problem, and each has bitten the project
already per the docs.

- Pair selection for [`rejudge-pair`](../docs/compiler/reconciler.md#pairs) uses
  stem and stop-word token overlap. The reconciler page records a five-document
  handbook deriving 113 pair goals off tokens like `s` and `their`. A score per
  candidate, "do these two statements bear on the same obligation", cuts the pair
  count before any session opens.
- The [`rejudge-pair`](../docs/compiler/goals/rejudge-pair.md) verdict itself is a
  three-way choice: `duplicate`, `contradiction`, `consistent`. Most pairs are
  consistent, and the `consistent` gate needs only a justification. The harness
  can close those itself with a templated justification carrying the probability,
  journaled as a machine verdict, and hand only acting verdicts and low-confidence
  pairs to a session. This is the largest tier 2 cost and the cleanest win.
- The lookalike score behind
  [`dedupe-candidates`](../docs/compiler/goals/dedupe-candidates.md) is Dice on
  name tokens plus Jaccard on documents. `docs/TODO.md` already reserves this slot
  for embeddings as "a similarity signal inside deterministic machinery, never
  the context path". A calibrated "same concept" boolean fits that slot better
  than cosine distance. A `separate` verdict at high confidence can be remembered
  in the existing `lookalikes` list without a session. A merge still goes to a
  session: a wrong merge destroys information.
- [`place-anchors`](../docs/compiler/goals/place-anchors.md) is, per anchor, a
  choice among at most three candidates plus a flag: place, place and
  re-evaluate, or homeless. It sits at tier 0 and blocks every other goal, so
  taking it off the session path shortens every incremental build.
- The "looks normative" heuristic behind `suspicious-non-normative`
  ([coverage](../docs/compiler/compilation.md#coverage)) is a word list: shall,
  provides, handles, definition bullets. A per-section boolean replaces it.
- Project [lint rules](../docs/compiler/project-settings.md#linting) are plain
  English and run only inside `review-entity` sessions. `docs/TODO.md` wants a
  checks-side sweep. A boolean per sentence per rule makes that sweep cheap
  enough to run on every build.
- `retrace`, `curate-view` membership, and `bind` site selection are choices over
  bounded candidate sets. Bind over a large deliverable needs elimination rounds,
  since option quality drops past about a hundred.
- Facet, edge type, stereotype, and transition classification are enumerated
  fields the chat model fills during extraction. Splitting them into a second
  call is not worth the round trip. A cheap semantic gate is: when a session
  stages an edge or a statement, ask "does this quote support this statement" and
  bounce or flag at high-confidence disagreement. The docs say the harness cannot
  judge whether a definition fits. A calibrated judge gives it a first opinion.

What stays with sessions: `reconcile-section` extraction, definition refresh,
`derive-criteria`, abstraction, generation, verification, answers, and every
acting verdict that needs a message or a mutation with a reason.

## The LSP and instant feedback

The [LSP](../docs/frontends/lsp.md) is read-only, never calls the LLM, and
repaints only when a build commits. Feedback today is minutes behind the
keystroke. At sub-second latency and near-zero cost an on-type path becomes
viable. On each changed section, debounced:

- normative detection and the lint rules,
- "is this sentence a statement the graph does not hold yet",
- "which existing entity does this unfamiliar noun phrase name", the glossary
  promotion idea in `docs/TODO.md`, as a choice over the entities in scope with a
  `none` option,
- "does this sentence conflict with any of the loaded statements on the entities
  it mentions", as a choice over the neighbor set with a `none` option,
- when a quote breaks, "which sentence is this now", the alignment preview.

Two constraints keep this honest with the graph invariants:

- On-type findings are provisional and never enter the store: no id, no journal,
  no provenance. They are hints in the editor, distinct from sticky judged
  diagnostics, and a build's real verdict supersedes them.
- The LSP page's "never calls the LLM" changes, so this is a docs-first edit,
  plus an opt-in setting: every keystroke would ship prose to a remote endpoint.

The GUI editor gets the same through the LSP.

## How it lands architecturally

A System One model is not an ACP agent. It cannot drive a tool loop, so it does
not slot into `[executors]`. The right shape is a seam like `render_svg`: a
judge trait with `choose`, `score`, and `decide`, one implementation over
TypeSafe's endpoint and one fallback over any OpenAI-compatible chat endpoint
using single-token constrained output with logprobs. The fallback keeps
LocalRouter and the local models in play and keeps the dependency policy intact.

- Deterministic machinery calls the judge with a confidence threshold. Above it
  the harness resolves or scores. Below it the goal falls to a session as today.
- Every judge verdict is keyed by an input hash and remembered, so a no-op
  rebuild still makes zero calls, and flip detection is not fed by a verdict
  oscillating around the threshold.
- A machine verdict journals as its own entry kind, so `jazyk explain` and the
  LSP log say a judge closed the goal, with the probability.
- Thresholds are registry constants in `limits.rs`, not settings, until
  dogfooding settles them.

## Risks

- Calibration is a vendor claim until the benchmark measures it. The deferred
  planted-ground-truth cases for `rejudge-pair` and `dedupe-candidates`
  ([deferred cases](../docs/benchmark/benchmark.md#deferred-cases)) are the
  prerequisite, and the benchmark needs a judge column beside the two codecs.
- Explainability thins. A machine verdict journals a probability, not a reason.
  Keeping acting verdicts in sessions limits this.
- Single vendor. The chat-model fallback is what makes the seam portable.
- Cost and privacy on the on-type path. Opt-in only, and per-action approval for
  any paid run.

## Order

1. The judge seam with cache and the chat-model fallback.
2. The benchmark cases that can grade it, with a judge column.
3. Pair pre-filtering and `consistent` auto-close on `rejudge-pair`.
4. Lookalike judging feeding `dedupe-candidates` and `review-entity`.
5. Checks-side lint and normative detection, published through the LSP at build
   time.
6. On-type provisional hints in the LSP.
7. `place-anchors` auto-resolution.
8. Later: the quote-supports-statement gate, bind site choice, flow clustering.

## Sources

- [System One concept page](https://docs.typesafe.ai/concepts/system-one)
- [Introducing System One Models and Jev](https://typesafe.ai/blog/introducing-system-one-models-and-jev)
- [Two techniques for working with System One models](https://www.seangoedecke.com/two-techniques-for-working-with-system-one-models/)
