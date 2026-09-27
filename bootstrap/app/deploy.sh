#!/bin/bash
# Publish jazyk.dev, the editor, to S3 + CloudFront.
#
#   Built by:     npm run build (dist/, docs/frontends/wasm.md#building)
#   S3 bucket:    jazyk.dev (us-east-1, private; CloudFront reads it through an OAC)
#   Distribution: the one whose alias is jazyk.dev, looked up at deploy time
#   Certificate:  ACM us-east-1, DNS-validated
#   DNS:          Porkbun, apex ALIAS + www CNAME to the CloudFront domain
#
# Vite hashes every asset, so the assets are cached for a week. What is fetched by
# name goes up uncached: index.html, the demo fixtures, the web manifest, and the
# service worker with its Workbox runtime, so a new build is one new precache
# manifest and the assets it names (docs/frontends/wasm.md#offline). The whole
# distribution is invalidated afterwards. hosting.sh creates the pieces; details in
# README.md.
#
# Locally this uses AWS_PROFILE=smotana. In GitHub Actions the OIDC role's
# credentials arrive in the environment, so the profile is left alone.

set -ex

if [ -z "${AWS_ACCESS_KEY_ID:-}" ]; then
  export AWS_PROFILE="${AWS_PROFILE:-smotana}"
fi

BUCKET=jazyk.dev

cd "$(dirname "$0")/dist"
test -f index.html
test -f sw.js

DISTRIBUTION_ID=$(aws cloudfront list-distributions \
  --query "DistributionList.Items[?contains(Aliases.Items || \`[]\`, '$BUCKET')].Id | [0]" --output text)
test "$DISTRIBUTION_ID" != "None"

UNCACHED=(--include '*.html' --include 'demos/*' --include 'sw.js' --include 'workbox-*.js' --include 'manifest.webmanifest')

aws s3 sync . "s3://$BUCKET/" --delete \
  --exclude '.DS_Store' --exclude '*.html' --exclude 'demos/*' --exclude 'sw.js' --exclude 'workbox-*.js' --exclude 'manifest.webmanifest' \
  --cache-control "max-age=604800"

aws s3 sync . "s3://$BUCKET/" \
  --exclude '*' "${UNCACHED[@]}" --exclude 'manifest.webmanifest' \
  --cache-control "max-age=0"

# The manifest by its own type: S3 guesses none for .webmanifest.
aws s3 cp manifest.webmanifest "s3://$BUCKET/manifest.webmanifest" \
  --content-type 'application/manifest+json' --cache-control "max-age=0"

aws cloudfront create-invalidation --distribution-id "$DISTRIBUTION_ID" --paths "/*"
