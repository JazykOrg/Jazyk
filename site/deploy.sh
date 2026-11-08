#!/bin/bash
# Publish jazyk.org, the static site in this directory, to S3 + CloudFront.
#
#   S3 bucket:    jazyk.org (us-east-1, private; CloudFront reads it through an OAC)
#   Distribution: E1KP0JBG5R7M1S (jazyk.org, www.jazyk.org)
#   Certificate:  ACM us-east-1, DNS-validated
#   DNS:          Porkbun, apex ALIAS + www CNAME to the CloudFront domain
#
# Nothing is built: the files here are the site. Everything but the HTML pages and
# schemas/ is cached for a week, the HTML pages and the schemas are not cached, both syncs delete what the
# directory no longer has, and the whole distribution is invalidated afterwards.
# Details in README.md.
#
# Locally this uses AWS_PROFILE=smotana. In GitHub Actions the OIDC role's
# credentials arrive in the environment, so the profile is left alone.

set -ex

if [ -z "${AWS_ACCESS_KEY_ID:-}" ]; then
  export AWS_PROFILE="${AWS_PROFILE:-smotana}"
fi

BUCKET=jazyk.org
DISTRIBUTION_ID=E1KP0JBG5R7M1S

cd "$(dirname "$0")"

aws s3 sync . "s3://$BUCKET/" --delete \
  --exclude 'deploy.sh' --exclude 'README.md' --exclude '.DS_Store' --exclude '*.html' \
  --exclude 'schemas/*' \
  --cache-control "max-age=604800"

# The HTML pages and the schemas change with every release: no cache.
aws s3 sync . "s3://$BUCKET/" --delete \
  --exclude '*' --include '*.html' --include 'schemas/*.json' \
  --cache-control "max-age=0"

aws cloudfront create-invalidation --distribution-id "$DISTRIBUTION_ID" --paths "/*"
