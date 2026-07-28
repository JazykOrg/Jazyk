# Site

The public site at [jazyk.org](https://jazyk.org). Plain static HTML with
[Tailwind CSS](https://tailwindcss.com) loaded from its CDN, so there is no
build step. Specified in [`../docs/site.md`](../docs/site.md).

## Pages

| File | Route |
| --- | --- |
| `index.html` | `/` |
| `compilation/index.html` | `/compilation` |
| `artifact/index.html` | `/artifact` |
| `favicon.svg` | `/favicon.svg` |
| `CNAME` | custom domain for GitHub Pages |

## Preview locally

```sh
cd site
python3 -m http.server 8000   # then open http://localhost:8000
```

## Deploy

Pushed to `master` and published to GitHub Pages by
[`.github/workflows/site.yml`](../.github/workflows/site.yml). No manual step.
