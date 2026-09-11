# Backups

Backups is the part of [Ops](ops.md) that keeps copies of the
[Database](database.md).

## Schedule

- Backups take a full snapshot of the Database primary every night at 02:00 UTC.
- Backups keep 30 daily snapshots and 12 monthly snapshots.
- Backups copy every snapshot to a second region within 1 hour of taking it.
- Backups encrypt every snapshot at rest.

## Restore

- Backups restore a snapshot into a fresh Database instance within 2 hours.
- Backups run a test restore of the latest snapshot once per week and record whether
  it succeeded.
- The [Cache](cache.md) and the [Queue](queue.md) are not backed up: the Cache is
  rebuilt from the Database, and a job lost with the Queue is enqueued again by hand.
