# Database

The Database is the PostgreSQL 16 instance in the [Backend](backend.md) that stores
every record the [API Server](api-server.md#data-model) owns. It runs as one primary and
one streaming replica. The [API Server](api-server.md) writes only to the primary and
may read from the replica.

## Tables

The Database has one table per class the API Server owns, named in snake case:
`accounts`, `organizations`, `members`, `sessions`, `api_keys`, `projects`, `tasks`,
`comments`, `attachments`, `notifications`, `audit_entries`, `plans`, `subscriptions`,
and `invoices`. Every table has an `id` primary key, a `created_at` column, and an
`updated_at` column. Every foreign key is declared as a constraint.

## Migrations

Schema changes are applied by migrations. Every migration is a numbered SQL file
committed with the code that needs it. [Deployment](deployment.md) applies pending
migrations before starting a new version of the API Server. A migration never drops a
column in the same release that stops writing it.

## Retention

The Database keeps `audit_entries` for 2 years and `sessions` for 30 days after their
expiry. Rows older than that are deleted by a nightly job on the [Queue](queue.md).
Every other table keeps its rows until the owning Organization is deleted.

## Protection

[Backups](backups.md) take a snapshot of the primary every night. The Database accepts
connections only over TLS and only from the API Server and the worker processes of the
Queue.
