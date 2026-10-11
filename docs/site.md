# Site

The public site for the project is hosted at [jazyk.org](https://jazyk.org).

## Style

- Static HTML, one hand-editable file per page. The home page uses Tailwind
  classes from its CDN script. Documentation pages share a stylesheet and a small
  script for copying examples and highlighting the current section. No build
  step, no framework, no bundler.
- Every image the site shows is a real rendering, copied into `site/` as a static asset
  from a built example's `.jazyk/`. A page never links into an example directory or
  into an out directory; links between pages and to assets are relative to the site.
- Output blocks are copied from real runs, condensed where a line would wrap.
  Setup commands are checked against the CLI. Instructional examples are labeled
  as examples and never presented as compiler output.

## Hosting

The site is served from a private S3 bucket through CloudFront, the same shape as the
maintainer's other static sites. [The editor](#the-editor) at jazyk.dev is hosted the
same way, with the differences listed there.

- The bucket is `jazyk.org` in us-east-1 with public access blocked. Only the CloudFront
  distribution reads it, through an origin access control; a direct request to the bucket
  returns 403.
- The distribution answers for `jazyk.org` and `www.jazyk.org` with an ACM certificate
  (us-east-1, DNS-validated, auto-renewing), redirects HTTP to HTTPS, and compresses.
- A CloudFront function on the viewer request maps `/` and any path without a file
  extension to its `index.html`.
- A missing object (S3 answers 403 through the origin access control) renders `404.html`
  with status 404.
- DNS is at Porkbun, the registrar: an ALIAS on the apex and a CNAME on `www`, both to the
  distribution's domain, plus the certificate's validation CNAME, which must stay.
- Deploying is `site/deploy.sh`: sync everything but HTML with a week of cache, the HTML
  files with no cache, then invalidate the distribution. The GitHub Actions workflow
  runs the same script on every push to `master` that touches `site/`, assuming an IAM
  role through OpenID Connect; no long-lived key is stored in GitHub.

## Pages

The home page and compiler documentation.

### Home (/)

- Hero: "Jazyk", pronunciation "/ˈjazɪk/", subtitle "English as a programming
  language".
- The pitch, in a paragraph: a compiler that turns documentation → requirements →
  acceptance criteria → architecture → design → source code → tests, one test per
  requirement. A change to the docs trickles down to the code, including migration,
  deployment, rollback. A problem downstream trickles up to the docs as a warning or
  an error: a hole, an ambiguity, a contradiction. The docs open in an IDE, each
  sentence linked to its requirement, design, code, and tests, the diagnostics
  inline. The compiler runs on the reader's own coding AI.
- One example, the sort utility (`example-sort`), as numbered sections one under the
  other:
  - Docs: the document, condensed.
  - Requirements and acceptance criteria: two requirements as the graph states them,
    each with its quote, and under each two criteria in the
    [criterion](./compiler/model/criterion.md) form (one condition, one outcome,
    concrete values), indented.
  - Architecture: three diagrams side by side. The sort utility is one entity, so
    the diagrams are the ledger example's, real renderings copied into the site: the
    top level's component view, the Checkout level's class view, and the Checkout
    sequence view.
  - Tests: the generated tests that assert the criteria above, named for their
    requirements.
  - Code: the generated program, condensed.
- A link to [the editor](#the-editor) to open the demos, start a project, or open a
  folder. A folder needs no existing configuration. Choose named document sets
  and the source files each describes in the editor.
- The ways to run it, under the link, one line each:
  - Demo: a ready-made project in the browser, no account.
  - Local folder: a folder on the computer, compiled by the reader's own coding AI,
    no account.
  - Cloud: projects kept online and compiled with included AI. GitHub sign-in starts
    a 14-day trial; paid plans continue it. Prices live in the editor's Plan & usage,
    never on this page, since plans are edited without a site deploy.
- Header and footer link to the documentation.
- The home page has no repository link. The CLI installation guide links to the
  public source repository.

### Documentation (`/docs/`)

The documentation teaches the compiler. Frontend setup is one part of it.

- Keep the entry page opening: `GETTING STARTED`, `Build from your words.`,
  and `Choose a workspace, write a first document, and learn the loop from intent
  to verified output.` A demo precedes workspace choices.
- The next page is Installation. It explains local and Cloud setup briefly.
  Local GUI, IDE, coding-agent, and Cloud guides are setup subpages.
- The primary reading path continues through document structure, writing
  requirements, the graph, generation, and verification.
- A second group covers incremental change, questions and provenance,
  diagrams, and adopting Jazyk in an existing project.
- Reference covers CLI commands and options, model selection and benchmarks,
  project files, and troubleshooting.
- The sidebar groups pages by learning stage. Setup subpages appear under
  Installation when the reader visits that part. Each article has section
  navigation, previous/next links, and related reading.
- On small screens the guide navigation is collapsible and the table of contents
  moves above the article. Desktop guide navigation scrolls independently when
  the viewport is short.
- Copy buttons announce success or select text when clipboard access fails.
  Navigation works with a keyboard. A skip link goes to the article. Reduced
  motion disables smooth scrolling. Articles work without JavaScript.
- Use labeled instructional examples. A running Orderly example connects prose,
  requirements, criteria, change, and evidence. It is not presented as compiler
  output. Existing example diagrams retain their actual project attribution.
- Explain software and non-software artifacts. The deliverable is the artifact
  the documents require, not a description of that artifact.
- No documentation search is needed for this initial set of pages. Grouped
  navigation and contextual links expose the complete collection.

### Getting started

- `/docs/`: choose a workspace and see the complete reading path.
- `/docs/installation/`: build the local binary, initialize a project, add a
  document set, and choose how to work. State AI prerequisites before compiling.
- `/docs/gui/`: use jazyk.dev with a local compiler or browser folder.
  State where files live and whether a model is ready.
- `/docs/ide/`: inline language support, the Jazyk chat agent, and their setup.
  Language support reads the graph and never compiles by itself.
- `/docs/agents/`: expose Jazyk tools to an existing coding agent or select a
  downstream compilation agent. Explain the distinction and named set scope.
- `/docs/cloud/`: create or import hosted projects, build, review, back up,
  and explicitly deliver to GitHub. Prices and availability live in Plan & usage.

### Learn the compiler

- `/docs/structure/`: document sets, roots, headings, links, input formats,
  and the separation of docs, source files, and compiler state.
- `/docs/writing/`: named subjects, obligations, boundaries, failure paths,
  measured qualities, constraints, and concrete examples. Show weak and revised
  prose without prescribing a controlled language.
- `/docs/graph/`: entities, requirements, criteria, derived relationships and
  state machines, views, diagnostics, provenance, and the compiler build loop.
- `/docs/product/`: bind → generate → verify. Explain source write scope,
  artifact medium, existing implementations, and explicit delivery.
- `/docs/verification/`: evidence per criterion, programmatic and model tests,
  failure and stale evidence, coverage, and limits of convergence.

### Work with change

- `/docs/changes/`: incremental reconciliation, preserved identities,
  stale evidence, causality, journal, and preview. Include a clearly labeled
  interactive illustration of how different edits affect the project.
  It illustrates the contract and does not run a compiler.
- `/docs/decisions/`: contradictions, ambiguity, human answers, quoted and
  proposed facts, ratification, and durable diagnostic triage.
- `/docs/diagrams/`: diagrams as graph projections, view types, containment,
  lifted arrows, collapse, and PlantUML as document input.
- `/docs/existing/`: adopt one part of an existing project, bind to its current
  implementation, and use decompilation drafts with human review.

### Reference

- `/docs/cli/`: commands by task, selectors, trace options, bounded sessions,
  and automation exit codes. Link setup back to Installation.
- `/docs/models/`: embedded and external agents, endpoint settings, resolution
  precedence, benchmark codecs, model quality, and practical cost control.
- `/docs/project/`: current `.jazyk/jazyk.toml` schema, named sets, matching,
  source scopes, generated state, global configuration, and Git tracking.
- `/docs/troubleshooting/`: symptom-led recovery for missing sets, unsupported
  files, incomplete builds, stale evidence, unavailable models, and editor refresh.

### 404

- Any path with no object renders `404.html` with status 404 and a link back.

## The editor

[jazyk.dev](https://jazyk.dev) is the Jazyk Cloud application. Its complete editor,
PWA, browser storage, and deployment live in the separate private
[Jazyk Cloud repository](https://github.com/JazykOrg/jazyk-cloud).
The application preserves accountless local and browser use alongside future
hosted projects. It uses this repository's native or WASM compiler engine.

[`jazyk gui`](./frontends/gui.md#serving) starts a local API and opens jazyk.dev.
The API and IDE interfaces remain independent engine features. The website move
does not change the local project's storage authority or require a Cloud account.

The private repository owns [editor deployment](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/deployment.md).
Its local extraction is reviewed before a live-site handoff. This repository's
`site/deploy.sh` and site workflow own jazyk.org only.
