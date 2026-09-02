# Monitoring

Monitoring is the part of [Ops](ops.md) that watches the running services and pages the
on-call engineer when something is wrong.

## Signals

- Monitoring collects the request logs of the [API Server](api-server.md) and computes
  the error rate and the latency per minute.
- Monitoring checks `/healthz` on every replica of the API Server every 30 seconds.
- Monitoring records the depth of the [Queue](queue.md) and the age of its oldest
  `queued` job every minute.
- Monitoring records the replication lag of the [Database](database.md) every minute.

## Alerts

- Monitoring pages the on-call engineer when the error rate of the API Server exceeds 1
  percent over 5 minutes.
- Monitoring pages the on-call engineer when a health check fails 3 times in a row on
  every replica.
- Monitoring pages the on-call engineer when the oldest `queued` job is older than 30
  minutes.
- Monitoring pages the on-call engineer when the replication lag of the Database
  exceeds 60 seconds.
- Monitoring opens a ticket, without paging, when a job is marked `failed`.

## Retention

Monitoring keeps request logs for 14 days and metrics for 13 months.
