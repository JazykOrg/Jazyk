#!/bin/bash
# Create the hosting for jazyk.dev, the same shape as jazyk.org (site/README.md):
# a private S3 bucket read by a CloudFront distribution through an origin access
# control, an ACM certificate validated over DNS, a viewer function that maps
# extensionless paths to index.html, a response headers policy that makes the
# page cross-origin isolated (docs/frontends/wasm.md#serving), DNS at Porkbun,
# and the deploy role's permission to sync the bucket and invalidate the
# distribution. Every step is skipped when its piece already exists, so the
# script can be rerun to finish a partial setup.
#
# Needs AWS_PROFILE=smotana and `source ~/bin/porkbun-env`.

set -euo pipefail
export AWS_PROFILE="${AWS_PROFILE:-smotana}"
export AWS_DEFAULT_REGION=us-east-1

DOMAIN=jazyk.dev
ACCOUNT=750984813907
ROLE=jazyk-site-deploy
ORG_DISTRIBUTION=E1KP0JBG5R7M1S

porkbun() {
  curl -s -X POST "https://api.porkbun.com/api/json/v3/dns/$1" -H 'Content-Type: application/json' \
    -d "{\"apikey\":\"$PORKBUN_API_KEY\",\"secretapikey\":\"$PORKBUN_SECRET_API_KEY\"${2:+,$2}}"
}

# The certificate, and its validation record at Porkbun.
CERT=$(aws acm list-certificates --query "CertificateSummaryList[?DomainName=='$DOMAIN'].CertificateArn | [0]" --output text)
if [ "$CERT" = "None" ]; then
  CERT=$(aws acm request-certificate --domain-name "$DOMAIN" --subject-alternative-names "*.$DOMAIN" \
    --validation-method DNS --query CertificateArn --output text)
  sleep 5
fi
VALIDATION=$(aws acm describe-certificate --certificate-arn "$CERT" \
  --query 'Certificate.DomainValidationOptions[0].ResourceRecord.[Name,Value]' --output text)
V_NAME=$(echo "$VALIDATION" | cut -f1 | sed "s/\.$DOMAIN\.$//")
V_VALUE=$(echo "$VALIDATION" | cut -f2 | sed 's/\.$//')
if ! porkbun "retrieve/$DOMAIN" | grep -q "$V_NAME"; then
  porkbun "create/$DOMAIN" "\"name\":\"$V_NAME\",\"type\":\"CNAME\",\"content\":\"$V_VALUE\",\"ttl\":\"600\""
fi
until [ "$(aws acm describe-certificate --certificate-arn "$CERT" --query Certificate.Status --output text)" = ISSUED ]; do
  echo "waiting for the certificate"; sleep 20
done

# The bucket, private.
if ! aws s3api head-bucket --bucket "$DOMAIN" 2>/dev/null; then
  aws s3api create-bucket --bucket "$DOMAIN"
  aws s3api put-public-access-block --bucket "$DOMAIN" \
    --public-access-block-configuration BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
fi

# The origin access control.
OAC=$(aws cloudfront list-origin-access-controls --query "OriginAccessControlList.Items[?Name=='$DOMAIN.s3.us-east-1.amazonaws.com'].Id | [0]" --output text)
if [ "$OAC" = "None" ]; then
  OAC=$(aws cloudfront create-origin-access-control --origin-access-control-config \
    "Name=$DOMAIN.s3.us-east-1.amazonaws.com,Description=$DOMAIN web editor,SigningProtocol=sigv4,SigningBehavior=always,OriginAccessControlOriginType=s3" \
    --query OriginAccessControl.Id --output text)
fi

# The viewer function.
FN=jazyk-dev-dir-index
if ! aws cloudfront describe-function --name "$FN" >/dev/null 2>&1; then
  cat > "/tmp/$FN.js" <<'JS'
function handler(event) {
  var request = event.request;
  var uri = request.uri;
  if (uri.endsWith("/")) {
    request.uri = uri + "index.html";
  } else if (!uri.split("/").pop().includes(".")) {
    request.uri = uri + "/index.html";
  }
  return request;
}
JS
  ETAG=$(aws cloudfront create-function --name "$FN" --function-code "fileb:///tmp/$FN.js" \
    --function-config "Comment=$DOMAIN: map / and extensionless paths to index.html,Runtime=cloudfront-js-2.0" --query ETag --output text)
  aws cloudfront publish-function --name "$FN" --if-match "$ETAG" >/dev/null
fi
FN_ARN="arn:aws:cloudfront::$ACCOUNT:function/$FN"

# The response headers policy: cross-origin isolation.
RHP=$(aws cloudfront list-response-headers-policies --type custom \
  --query "ResponseHeadersPolicyList.Items[?ResponseHeadersPolicy.ResponseHeadersPolicyConfig.Name=='jazyk-dev-isolation'].ResponseHeadersPolicy.Id | [0]" --output text)
if [ "$RHP" = "None" ]; then
  RHP=$(aws cloudfront create-response-headers-policy --response-headers-policy-config '{
    "Name": "jazyk-dev-isolation",
    "Comment": "jazyk.dev: cross-origin isolation so the browser backend gets SharedArrayBuffer",
    "CustomHeadersConfig": {"Quantity": 2, "Items": [
      {"Header": "Cross-Origin-Opener-Policy", "Value": "same-origin", "Override": true},
      {"Header": "Cross-Origin-Embedder-Policy", "Value": "require-corp", "Override": true}]}}' \
    --query ResponseHeadersPolicy.Id --output text)
fi

# The distribution. A missing object is a 403 through the OAC; the editor is one
# page with its routes in the fragment, so every such path serves index.html.
DIST=$(aws cloudfront list-distributions --query "DistributionList.Items[?contains(Aliases.Items || \`[]\`, '$DOMAIN')].Id | [0]" --output text)
if [ "$DIST" = "None" ]; then
  cat > "/tmp/$DOMAIN-distribution.json" <<JSON
{
  "CallerReference": "$DOMAIN-$(date +%s)",
  "Comment": "$DOMAIN web editor (bootstrap/app demo build in JazykOrg/Jazyk)",
  "Enabled": true,
  "DefaultRootObject": "index.html",
  "Aliases": {"Quantity": 2, "Items": ["$DOMAIN", "www.$DOMAIN"]},
  "Origins": {"Quantity": 1, "Items": [{
    "Id": "$DOMAIN.s3.us-east-1.amazonaws.com",
    "DomainName": "$DOMAIN.s3.us-east-1.amazonaws.com",
    "OriginPath": "",
    "CustomHeaders": {"Quantity": 0},
    "S3OriginConfig": {"OriginAccessIdentity": "", "OriginReadTimeout": 30},
    "ConnectionAttempts": 3,
    "ConnectionTimeout": 10,
    "OriginShield": {"Enabled": false},
    "OriginAccessControlId": "$OAC"
  }]},
  "DefaultCacheBehavior": {
    "TargetOriginId": "$DOMAIN.s3.us-east-1.amazonaws.com",
    "ViewerProtocolPolicy": "redirect-to-https",
    "AllowedMethods": {"Quantity": 2, "Items": ["HEAD", "GET"], "CachedMethods": {"Quantity": 2, "Items": ["HEAD", "GET"]}},
    "Compress": true,
    "FunctionAssociations": {"Quantity": 1, "Items": [{"FunctionARN": "$FN_ARN", "EventType": "viewer-request"}]},
    "CachePolicyId": "658327ea-f89d-4fab-a63d-7e88639e58f6",
    "OriginRequestPolicyId": "88a5eaf4-2fd4-4709-b370-b4c650ea3fcf",
    "ResponseHeadersPolicyId": "$RHP"
  },
  "CacheBehaviors": {"Quantity": 0},
  "CustomErrorResponses": {"Quantity": 1, "Items": [{"ErrorCode": 403, "ResponsePagePath": "/index.html", "ResponseCode": "200", "ErrorCachingMinTTL": 10}]},
  "ViewerCertificate": {"ACMCertificateArn": "$CERT", "SSLSupportMethod": "sni-only", "MinimumProtocolVersion": "TLSv1.2_2021"},
  "HttpVersion": "http2and3",
  "PriceClass": "PriceClass_100",
  "IsIPV6Enabled": true
}
JSON
  DIST=$(aws cloudfront create-distribution --distribution-config "file:///tmp/$DOMAIN-distribution.json" --query Distribution.Id --output text)
fi
CF_DOMAIN=$(aws cloudfront get-distribution --id "$DIST" --query Distribution.DomainName --output text)
echo "distribution $DIST at $CF_DOMAIN"

# The bucket policy: only this distribution reads.
aws s3api put-bucket-policy --bucket "$DOMAIN" --policy "{
  \"Version\": \"2012-10-17\",
  \"Statement\": [{
    \"Sid\": \"AllowCloudFrontServicePrincipal\",
    \"Effect\": \"Allow\",
    \"Principal\": {\"Service\": \"cloudfront.amazonaws.com\"},
    \"Action\": \"s3:GetObject\",
    \"Resource\": \"arn:aws:s3:::$DOMAIN/*\",
    \"Condition\": {\"StringEquals\": {\"AWS:SourceArn\": \"arn:aws:cloudfront::$ACCOUNT:distribution/$DIST\"}}
  }]
}"

# The deploy role syncs both buckets and invalidates both distributions; listing
# distributions is how deploy.sh finds this one by its alias.
aws iam put-role-policy --role-name "$ROLE" --policy-name site-deploy --policy-document "{
  \"Version\": \"2012-10-17\",
  \"Statement\": [
    {\"Sid\": \"SyncSite\", \"Effect\": \"Allow\",
     \"Action\": [\"s3:ListBucket\", \"s3:GetBucketLocation\", \"s3:GetObject\", \"s3:PutObject\", \"s3:DeleteObject\"],
     \"Resource\": [\"arn:aws:s3:::jazyk.org\", \"arn:aws:s3:::jazyk.org/*\", \"arn:aws:s3:::$DOMAIN\", \"arn:aws:s3:::$DOMAIN/*\"]},
    {\"Sid\": \"InvalidateCloudFront\", \"Effect\": \"Allow\",
     \"Action\": [\"cloudfront:CreateInvalidation\", \"cloudfront:GetInvalidation\", \"cloudfront:ListInvalidations\"],
     \"Resource\": [\"arn:aws:cloudfront::$ACCOUNT:distribution/$ORG_DISTRIBUTION\", \"arn:aws:cloudfront::$ACCOUNT:distribution/$DIST\"]},
    {\"Sid\": \"FindDistribution\", \"Effect\": \"Allow\", \"Action\": \"cloudfront:ListDistributions\", \"Resource\": \"*\"}
  ]
}"

# DNS: the apex ALIAS and the www CNAME, both to the distribution.
RECORDS=$(porkbun "retrieve/$DOMAIN")
echo "$RECORDS" | grep -q "\"name\":\"$DOMAIN\",\"type\":\"ALIAS\"" || \
  porkbun "create/$DOMAIN" "\"name\":\"\",\"type\":\"ALIAS\",\"content\":\"$CF_DOMAIN\",\"ttl\":\"600\""
echo "$RECORDS" | grep -q "\"name\":\"www.$DOMAIN\",\"type\":\"CNAME\"" || \
  porkbun "create/$DOMAIN" "\"name\":\"www\",\"type\":\"CNAME\",\"content\":\"$CF_DOMAIN\",\"ttl\":\"600\""
echo "done: https://$DOMAIN"
