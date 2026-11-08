# Plan: the product, its versions, and its deployment

Status: proposal. Written 2026-09-29. Phase 1 (show the artifact) is implemented and
documented ([overview](../docs/frontends/gui.md#overview),
[the demo](../docs/frontends/wasm.md#the-demo)); the rest is not scheduled. The
docs-first rule applies: every phase below names the pages that change before the
code does.

Read with [generation](../docs/consumers/gen.md), [binding](../docs/consumers/bind.md),
[the control plane](../docs/compiler/control-plane.md), the GUI's
[Product lens](../docs/frontends/gui.md#product), and
[the demo](../docs/frontends/wasm.md#the-demo). Related proposal:
`plans/upstream-reconciliation.md` (edits to the product flowing back into prose).

## The owner's ask

Everything the graph produces is the product: source code, tests, a built binary, a
novel, a slide deck. Keep it in one place, the Product lens, for now. Later a
deployment state comes out of it too, and deployment matters: a document change to a
product with a database cannot simply be deployed, it follows a migration path and
keeps a rollback. Sketch how all of it works and the best path forward.

## Today

- The product is one deliverable directory and one build. The build (`run`, `cwd`,
  `produces`) is per deliverable; `jazyk gen` ends by running it and `jazyk test` runs
  it before judging ([the build](../docs/consumers/gen.md#the-build)).
- The ledger holds the last state only: files, sites, tests, verdicts, `build.lastRun`.
  The deliverable baseline keeps one run's before-image for the GUI diff. Nothing
  records a product version or a build output over time.
- A demo carries the product's sources and the ledger, never a built artifact: any
  file that is not UTF-8 text is left out ([the demo](../docs/frontends/wasm.md#the-demo)).
  The novel is never assembled; the deck's `.pptx` never reaches the tab.
- "Release" means an approval gate on model work
  ([modes and releases](../docs/compiler/control-plane.md#modes-and-releases)), a
  journal diff (pm.md), or jazyk's own release note. None is a product release.
- "Migration" means only the store's version rule ("no migration code, ever"), and
  "deployment" only a diagram kind placing artifacts in nodes from prose.
- The site promises "a change to the docs trickles down to the code, including
  migration, deployment, rollback". No page defines any of the three.
- The TODO list holds one owner decision on the way there: a start or smoke command
  for a software deliverable, so a row cannot read verified while the product does not
  start.

## The model

Four layers come out of the graph. Each has an owner, a record, and a place in the
Product lens.

| Layer | What it is | Who makes it | Record |
|---|---|---|---|
| Sources | the deliverable's files: code, chapters, the deck builder | `generate` sessions | ledger `entities.*.files` |
| Tests | the bound criteria's tests and llm criteria files | `bind` sessions | ledger `requirements.*.test(s)` |
| Artifacts | what the build produces: a binary, an image, a `.pptx`, a PDF | the build command | new: `artifacts` per version |
| Deployments | an artifact running somewhere, with state that outlives it | a deploy, released by a human | new: `deployments` per environment |

The first three exist and are derived: a document change moves them without a person
in the loop beyond the existing releases. The fourth is different in kind. It holds
state the graph does not own (a database's rows, a reader's saved progress, links
others keep), so it never follows the graph blindly. It follows a plan a person
approves.

### Product versions

A product version is one consistent point: the graph generation it was generated
from, the deliverable tree's content hash, the artifacts' hashes, and the test
verdicts at that point. A version is recorded when a `gen` run ends with its build
green (and optionally when `test` ends), as `gen/versions/v<N>.yaml` beside the ledger,
with the artifacts stored once by content hash under `gen/artifacts/<hash>`.

- Versions give the Product lens a history: what the product was after build #12,
  a diff between two versions (files, artifacts, verdicts), and `Restore` of a
  version's sources into the deliverable (a regeneration from an older graph is not
  needed: the files are kept).
- They are the unit a deployment names: environment `prod` runs version 14.
- Retention: the newest few and every deployed version; older ones are pruned by the
  GC sweep, since the graph and the journal can regenerate them.

### State and migrations

Some entities describe things whose instances persist: a stored table, a file format
users keep, an API others call, a URL others link. Call them persistent. The graph
can know which ones: a stereotype («store», «database», «api»), a statement that
something is kept or stored, or a relationship to a store.

- When a generation changes a persistent entity's shape (its attributes, its
  relations, its format), the harness opens a `migrate` goal beside `generate`,
  scoped to the transition from the deployed or previous version to the new one.
- A migration is a deliverable file of its own kind: append-only, never rewritten
  once a version that includes it is deployed, owned by the transition rather than by
  an entity (the way database migrations are ordered files). Each carries a forward
  step and, when the change allows it, a backward step.
- Migrations have tests like any requirement: bind writes a migration test that loads
  the previous version's fixture data, runs the forward step, and checks the criteria
  the documents state about keeping data ("an order keeps its total after the
  currency change"). A backward step is tested the same way, in reverse.
- An irreversible change (a field dropped with data in it, a format no longer
  readable) is not decided by the model: it becomes a prompted diagnostic in the
  Inbox. "Removing `Customer.fax` deletes the stored values. Keep them, archive them,
  or drop them?" The answer lands in prose, like every answer, so the documents say
  what happens to the data.
- Non-software products have the same shape with lighter steps: a handbook site's
  moved pages need redirects (a migration), a novel's reader bookmarks need a chapter
  map, a deck has none.

### Deployments

An environment is a place a version runs: `local`, `staging`, `prod` for software;
a site, a store, a share for a book or a deck.

- Environments come from the documents, since docs-first applies to them too: a
  statement like "The shop is deployed in the EU region" already derives the
  deployment view. What the prose cannot hold (credentials, account ids, hostnames)
  lives in settings (`[deploy.<env>]` in `jazyk.toml` or the global config), never in
  the graph.
- A deploy is always a human release, recorded in the control plane
  (`released.deploy.<env> = v<N>`), never automatic by default. It runs in four steps:
  1. Plan: the difference between the version the environment runs and the candidate:
     files, artifacts, the migrations to run in order, and every irreversible step
     called out. The plan is a page the person reads before approving.
  2. Apply: run the migrations, then switch the artifact, through the project's own
     deploy command (`[deploy.<env>] run`), which generation writes as a support file
     (a script, a CI workflow) rather than jazyk calling cloud APIs itself.
  3. Check: the smoke command from the TODO decision runs against the environment. A
     failing check stops the rollout and offers rollback.
  4. Record: the environment's deployed version, the time, the migrations applied,
     journaled like a commit so History shows it.
- Rollback: switch the artifact back to the previous deployed version and run the
  backward steps of the migrations applied since, newest first. A migration with no
  backward step blocks an automatic rollback and says so, offering the restore the
  documents specify (a backup taken before the irreversible step, which the plan
  schedules).
- A document change that touches a deployed persistent entity reaches production only
  through this path: the change regenerates sources and opens `migrate`; the build and
  the tests (migration tests included) go green; a person approves the plan.

## The Product lens, when it is all there

- Overview, first: the product as it is now. A written product reads as one document
  in order (the novel's chapters assembled, the handbook's pages), with the browser's
  print for a PDF; a built product shows its artifact (the deck's slides as images the
  build exports, a download of the `.pptx`); a program shows its README, how to run
  it, and the test summary.
- Sources, Tests, and Trace, as today.
- Artifacts: each version's build outputs, downloadable.
- Versions: the history, a diff of two, Restore.
- Deployments: each environment, the version it runs, the pending plan with its
  migrations and irreversible steps, Deploy and Roll back.

## The best path forward

Each phase stands on its own and is useful before the next.

1. Show the artifact (small, now). Demo fixtures carry binary files (base64 in the
   fixture, decoded into the tab's tree) so a built artifact reaches the tab. The
   Product lens gains Overview: a written product assembled in reading order with
   print to PDF, a built product's `produces` shown or offered for download, a program's
   README and test summary. The deck renders from its `.pptx` in the page, through a
   PowerPoint renderer loaded on first use, so the build exports no PNG. Pages:
   gui.md#product, wasm.md#the-demo.
2. Product versions. `gen/versions/` and content-addressed artifacts, a version
   recorded when a gen run ends green, the Versions view with diff and Restore, the GC
   prune. Pages: gen.md#the-ledger (a new section), graph.md#garbage-collection,
   gui.md#product.
3. The product runs. Settle the TODO's smoke command: the medium records a start and a
   smoke command for a software deliverable; `jazyk test` runs the smoke check; a row
   cannot read verified while it fails. Pages: gen.md#the-medium-is-decided-once-before-anything-is-generated,
   gen.md#runners, TODO.md.
4. Persistence and migrations. The persistent marker on entities, the `migrate` goal
   kind with its contract and gate, migration files as append-only deliverable files,
   migration tests from the previous version's fixture data, irreversible changes as
   prompted diagnostics. Pages: a new docs/compiler/goals/migrate.md,
   docs/compiler/model/entity.md, gen.md, bind.md, reconciler.md (derivation and
   tier), the skills.
5. Deployments. Environments from prose plus `[deploy.<env>]` settings, the deploy
   release, plan, apply through the generated deploy command, check, record, rollback,
   and the Deployments view. Pages: a new docs/consumers/deploy.md,
   control-plane.md#modes-and-releases, project-settings.md, gui.md#product, cli.md
   (`jazyk deploy <env>`, `jazyk rollback <env>`).
6. Publishing for non-software products: environments as channels (a site, a store),
   the same plan and record, redirects and chapter maps as their migrations.

Phase 1 is a few days and fixes what the owner sees today. Phases 2 and 3 are the
foundation the rest needs. Phases 4 and 5 are the substance of "migration,
deployment, rollback" and should wait until 2 and 3 are in use on the examples: the
ERP (PostgreSQL) is the natural first case for migrations, the shop for a deploy.

## Open questions for the owner

- Does jazyk run deploys itself, or only generate the project's deploy pipeline (a
  script, a CI workflow) and record what was deployed? Recommended: generate and
  record; `jazyk deploy` runs the generated command locally, and a CI pipeline runs
  the same command, so jazyk never holds cloud credentials beyond what the command
  uses.
- Are environments stated in prose, in settings, or both? Recommended: both, split as
  above: where and why in prose, how to reach it in settings.
- Is a version recorded on every green `gen`, or only on an explicit "cut a version"?
  Recommended: every green gen (cheap, content-addressed), with deploys naming one.
- Should a rename in the documents (an entity renamed) be a migration for a
  persistent entity? Recommended: yes, since stored data and links keep the old name.
- Is the product word "Product" kept for all four layers in the GUI, with Deployments
  as its own section, or does Deployment become a fourth lens? Recommended: keep one
  Product lens until deployments exist, then decide from use.
