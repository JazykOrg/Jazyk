# Site

The public site at [jazyk.org](https://jazyk.org). Plain static HTML with
[Tailwind CSS](https://tailwindcss.com) loaded from its CDN, so there is no
build step. Specified in [`../docs/site.md`](../docs/site.md).

## Pages

| File | Route |
| --- | --- |
| `index.html` | `/`, the one page: the pitch and the sort example's snippets |
| `example/*.svg` | the diagrams on the page |
| `favicon.svg` | `/favicon.svg` |
| `schemas/*.json` | `/schemas/<name>.json`, the JSON Schemas generated from `docs/**/*.schema.yaml` ([`docs/site.md#schemas`](../docs/site.md#schemas)); regenerate with `JAZYK_WRITE_SCHEMAS=1 cargo test --lib site_schemas` in `bootstrap/` |
| `404.html` | served with status 404 for any path that has no object |
| `deploy.sh` | the publishing pipeline, not uploaded |

## Snippets and assets

The example blocks on the page are files of `example-sort` (its documents, its
`code/`, and its `.jazyk/graph/` shards), condensed where a line would wrap. The
acceptance criteria are written for the page in the criterion form, since the example
has no criteria shard built. `example/*.svg` are renderings copied from the
`example-ledger` project's built `.jazyk/diagrams/` (`component/public`,
`class/checkout`, `sequence/checkout-checkout`) with the drill-down anchors removed.
Re-copy them after a rebuild of an example changes them; the site never links into an
example directory.

## Preview locally

```sh
cd site
python3 -m http.server 8000   # then open http://localhost:8000
```

## Hosting

S3 + CloudFront in AWS account `750984813907` (`AWS_PROFILE=smotana`), the same shape as
the maintainer's other static sites (`zisk.ca`, `ollisten.com`, `matusnomin.com`).
Specified in [`../docs/site.md`](../docs/site.md#hosting).

| Piece | Value |
| --- | --- |
| S3 bucket | `jazyk.org`, us-east-1, private: public access blocked, no website config |
| Distribution | `E1KP0JBG5R7M1S` → `d2okr90n76jgwn.cloudfront.net` |
| Aliases | `jazyk.org`, `www.jazyk.org` |
| Origin access | OAC `EZQ94VF691MEH` (sigv4, always); the bucket policy allows only `cloudfront.amazonaws.com` for this distribution |
| Certificate | ACM us-east-1 `13cc059d-aafb-41ec-ab5c-7ac0608424b7` for `jazyk.org` and `*.jazyk.org`, DNS-validated, auto-renews |
| Viewer function | `jazyk-org-dir-index`: `/` and extensionless paths get `index.html` appended |
| Cache policy | Managed-CachingOptimized, compression on |
| Errors | 403 from S3 (a missing key, since the OAC has no `ListBucket`) → `/404.html` with status 404 |
| Deploy role | `jazyk-site-deploy`, assumed by GitHub Actions through OIDC for `JazykOrg/Jazyk` on `master` only |

The bucket is reachable only through CloudFront.

[jazyk.dev](https://jazyk.dev), the complete browser editor and Cloud application,
lives in the private [Jazyk Cloud repository](https://github.com/JazykOrg/jazyk-cloud)
under `web/`. That repository owns its build, PWA, browser tests, and eventual
publication. Its [deployment specification](https://github.com/JazykOrg/jazyk-cloud/blob/master/docs/editor/deployment.md)
defines the separate ownership handoff. This repository and `site/deploy.sh` own
jazyk.org only. Moving the editor source changes neither live domain.

## DNS

At Porkbun (the registrar is also the nameserver; there is no Route 53 zone for this
domain and there should not be one):

| Record | Type | Value |
| --- | --- | --- |
| `jazyk.org` | ALIAS | `d2okr90n76jgwn.cloudfront.net` |
| `www.jazyk.org` | CNAME | `d2okr90n76jgwn.cloudfront.net` |
| `_4d6d600ed77aa610d0ad91ee96c15edd` | CNAME | ACM validation, do not delete: the certificate stops renewing without it |

Porkbun's ALIAS flattens at the edge, which is what lets the apex point at CloudFront.
To change DNS from a script: `source ~/bin/porkbun-env`, then POST to
`https://api.porkbun.com/api/json/v3/dns/...` with `apikey` and `secretapikey` in the JSON
body. The apex record's `name` is the empty string, not `@`.

## Deploy

```sh
./deploy.sh
```

Everything but `*.html` goes up with `max-age=604800`, the HTML files with `max-age=0`,
both with `--delete`, then the distribution is invalidated, so a deploy is visible within
seconds.
[`.github/workflows/site.yml`](../.github/workflows/site.yml) runs the same script on
every push to `master` that touches `site/`, with credentials from the OIDC deploy role.
