#!/bin/bash
# Publish jazyk.dev, the editor in the browser, to S3 + CloudFront.
#
#   Built by:     npm run build:demo (dist-demo/, docs/frontends/wasm.md#building)
#   S3 bucket:    jazyk.dev (us-east-1, private; CloudFront reads it through an OAC)
#   Distribution: the one whose alias is jazyk.dev, looked up at deploy time
#   Certificate:  ACM us-east-1, DNS-validated
#   DNS:          Porkbun, apex ALIAS + www CNAME to the CloudFront domain
#
# Vite hashes every asset, so the assets are cached for a week and only
# index.html and the demo fixtures (fetched by name) go up uncached. The whole
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

cd "$(dirname "$0")/dist-demo"
test -f index.html

DISTRIBUTION_ID=$(aws cloudfront list-distributions \
  --query "DistributionList.Items[?contains(Aliases.Items || \`[]\`, '$BUCKET')].Id | [0]" --output text)
test "$DISTRIBUTION_ID" != "None"

aws s3 sync . "s3://$BUCKET/" --delete \
  --exclude '.DS_Store' --exclude '*.html' --exclude 'demos/*' \
  --cache-control "max-age=604800"

aws s3 sync . "s3://$BUCKET/" \
  --exclude '*' --include '*.html' --include 'demos/*' \
  --cache-control "max-age=0"

aws cloudfront create-invalidation --distribution-id "$DISTRIBUTION_ID" --paths "/*"
