# Site

The public site at [jazyk.org](https://jazyk.org). Plain static HTML with
[Tailwind CSS](https://tailwindcss.com) loaded from its CDN, so there is no
build step. Specified in [`../docs/site.md`](../docs/site.md).

## Pages

| File | Route |
| --- | --- |
| `index.html` | `/` |
| `compilation/index.html` | `/compilation` |
| `levels/index.html` | `/levels` |
| `graph/index.html` | `/graph` |
| `artifact/index.html` | `/artifact`, redirects to `/graph` |
| `favicon.svg` | `/favicon.svg` |
| `404.html` | served with status 404 for any path that has no object |
| `deploy.sh` | the publishing pipeline, not uploaded |

## Assets

`levels/*.svg` are renderings copied from the `example-ledger` project's built
`jazyk-out/diagrams/` (`component/public`, `class/checkout`, `class/funds`,
`sequence/checkout-checkout`), with the drill-down anchors rewritten to point down
the levels page instead of into the out directory. Re-copy them after a rebuild of
the example changes the pictures; the site never links into an example directory.

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

Everything but `*.html` goes up with `max-age=604800` and `--delete`, the HTML files with
`max-age=0`, then the distribution is invalidated, so a deploy is visible within seconds.
[`.github/workflows/site.yml`](../.github/workflows/site.yml) runs the same script on
every push to `master` that touches `site/`, with credentials from the OIDC deploy role.
