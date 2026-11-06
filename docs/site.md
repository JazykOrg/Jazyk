# Site

The public site for the project is hosted at [jazyk.org](https://jazyk.org).

## Style

- Static HTML, one hand-editable file per page, styled with Tailwind classes from its
  CDN script. No build step, no framework, no bundler.
- Every image the site shows is a real rendering, copied into `site/` as a static asset
  from a built example's `.jazyk/`. A page never links into an example directory or
  into an out directory; links between pages and to assets are relative to the site.
- Code blocks are real output: trace events, shard YAML, cards, and terminal lines are
  copied from a run, condensed where a line would wrap, never invented.

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

One page.

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
- No link to the repository.

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

## Native downloads

The site shows `npx -y jazyk` and `npm install -g jazyk`. It links each supported
platform archive through GitHub latest-release URLs, with checksums and all
releases beside them. [Distribution](frontends/distribution.md) defines the
platforms and the manual release workflow.
