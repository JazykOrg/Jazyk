# Cache

The Cache is the Redis instance in the [Backend](backend.md) that serves hot reads for
the [API Server](api-server.md). Everything in the Cache is a copy of a record in the
[Database](database.md), and the API Server may discard any entry at any time.

## Entries

- The API Server caches a [Session](identity.md#session) under the key
  `session:<token>` for the life of the Session, so a request checks its token without
  reading the Database.
- The API Server caches a [Project](collaboration.md#project) with its Tasks under the
  key `project:<id>` for 10 minutes.
- The API Server caches the list of [Plans](billing.md#plan) under the key `plans` for
  1 hour.

## Invalidation

- When a handler writes a Task, a Comment, or an Attachment, the API Server deletes the
  entry of the owning Project from the Cache.
- When a Session ends, the API Server deletes its entry from the Cache.
- When the Cache is unreachable, the API Server reads from the Database and keeps
  serving requests.

## Capacity

The Cache holds at most 2 GB and evicts the least recently used entry when full.
