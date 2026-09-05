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
| `CNAME` | custom domain for GitHub Pages |

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

## Deploy

Pushed to `master` and published to GitHub Pages by
[`.github/workflows/site.yml`](../.github/workflows/site.yml). No manual step.
