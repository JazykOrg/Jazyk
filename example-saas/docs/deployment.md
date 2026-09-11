# Deployment

Deployment is the part of [Ops](ops.md) that ships a new version to production. A
release ships the [Frontend](frontend.md) and the backend services together.

## Pipeline

- Every merge to the main branch builds one container image for each of the backend
  services and one static bundle of the Frontend.
- Deployment runs the test suite against the image before it is tagged; a failed test
  suite stops the release.
- Deployment applies pending [Database](database.md) migrations with `trellis migrate`
  before starting the new version of the [API Server](api-server.md).
- Deployment starts the new replicas of the API Server, waits until each answers
  `/healthz` with `200`, then stops the old replicas one at a time.
- Deployment publishes the Frontend bundle to the CDN after the new API Server replicas
  are serving.
- A release takes at most 15 minutes from merge to serving.

## Configuration

The backend services read their configuration from environment variables:
`TRELLIS_DATABASE_URL`, `TRELLIS_REDIS_URL`, `TRELLIS_MAIL_API_KEY`, and
`TRELLIS_PAYMENT_API_KEY`. Deployment sets them from the secrets store; they are never
committed to the repository. The containers write their logs to standard output, and
the files of each container live under `/srv/trellis/`.

## Rollback

Deployment keeps the previous image of each of the backend services. The on-call engineer rolls
a release back with `trellis rollback`, which restarts the previous image within 5
minutes. A migration is never rolled back; the previous version of the API Server runs
against the migrated schema.
