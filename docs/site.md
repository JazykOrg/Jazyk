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

The home page and four documentation pages.

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

- The entry page is `Start here`. It helps a developer or creator choose CLI,
  IDE, or Cloud by file location, AI setup, and account requirements.
- A demo is the shortest path to inspect an existing project. Local browser
  folders and the local CLI remain distinct choices.
- The common workflow is write → compile → review → generate → verify.
  Compilation builds the graph. Generation produces source or another
  deliverable. Verification evaluates its criteria. Cloud Build groups these
  steps when the set has work.
- Each guide states prerequisites, gives a first successful workflow, explains
  human decisions, and links to the next guide.
- The shell has the site header, persistent guide navigation, breadcrumbs,
  an on-page table of contents, and previous/next links. The active page and
  section are identified. On small screens the guide navigation is collapsible
  and the table of contents moves above the article.
- Examples have copy buttons with a success or failure message. Links, controls,
  and navigation work with a keyboard. A skip link goes to the article. Reduced
  motion preferences disable smooth scrolling. Content works without JavaScript.
- No documentation search is needed for four pages. Every guide is reachable
  in one click from the shared navigation.

### CLI (`/docs/cli/`)

- Build from source with Rust and Git, then put the binary on `PATH`.
- Initialize a project, write a document, and add a named document set.
  `jazyk init` creates `.jazyk/jazyk.toml` with an empty set list.
- Choose an installed coding agent or configure the embedded agent's endpoint.
  Keep credentials in the environment. Benchmark the chosen model.
- Compile, inspect the verdict, resolve questions, and recompile.
  `--set` selects a document set. Quote shell globs.
- Explain the everyday commands by task, including generation, verification,
  graph inspection, and the local browser editor.
- Explain where configuration, source, and generated state live.

### IDE (`/docs/ide/`)

- Start from a configured local project and a binary available to the IDE.
- Separate inline language support (LSP), the Jazyk chat agent (ACP), and tools
  used by an existing coding agent (MCP). Explain their purpose before naming
  the protocols.
- Give concrete setup paths for VS Code, Zed, JetBrains, Cursor, and coding
  agents. State any extension or adapter requirement.
- Show the edit, compile, inspect, and answer workflow. The language server
  reads the graph and does not compile on its own.
- Explain stale diagnostics, missing binary paths, and agent setup failures.

### Cloud (`/docs/cloud/`)

- Sign in with GitHub at jazyk.dev. Create a project or import from GitHub.
  Import and repository access are explicit steps.
- Explain document sets, Docs, Model, Product, Inbox, Build, and delivery.
- Cloud files and compilation live in the hosted service. Signing in does
  not move local files into Cloud.
- Explain trial eligibility and included AI. Link to the app's Plan & usage
  for current availability, prices, limits, and remaining usage.
- Explain backups, interrupted builds, and explicit GitHub delivery.
  Delivery requires review of the repository and branch before confirmation.
- Do not claim that hosted compilation of a local folder is available.

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
