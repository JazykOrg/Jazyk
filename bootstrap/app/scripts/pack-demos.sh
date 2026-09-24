#!/bin/sh
# Pack every example project into the fixtures the editor serves as demos. Run it
# after an example is rebuilt; the fixtures are committed, since the examples' out
# directories are not. See docs/frontends/wasm.md#the-demo.
set -e
cd "$(dirname "$0")/../../.."   # the repo root
node bootstrap/app/scripts/pack-demo.mjs bootstrap/app/public/demos \
  shop=bootstrap/example/f1 \
  sort=example-sort \
  erp=example-erp \
  slides=example-slides \
  novel=example-novel \
  org=example-org
