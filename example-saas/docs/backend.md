# Backend

The Backend is the set of services behind the [Frontend](frontend.md). It is made up of
four components:

- The [API Server](api-server.md), which handles every request from the Frontend.
- The [Database](database.md), which stores every record.
- The [Queue](queue.md), which runs background jobs.
- The [Cache](cache.md), which serves hot reads.

## Boundaries

Only the API Server accepts connections from outside the Backend. The Database, the
Queue, and the Cache accept connections only from the API Server and from the worker
processes of the Queue. Every connection between components of the Backend is
encrypted.

## Data ownership

The Database is the source of truth for every record. The Cache holds copies that the
API Server may discard at any time. The Queue holds jobs, never records: a job carries
the id of the record it works on and reads the record from the Database when it runs.

## Availability

The Backend answers 99.9 percent of requests each month. The Backend keeps serving reads
while the Queue is down. When the Database is unreachable, the API Server answers `503`
and the Frontend shows a maintenance notice.

## Deployment

The Backend runs as containers published by [Deployment](deployment.md). The API Server
runs in at least two replicas. The Database runs as one primary with one replica.
