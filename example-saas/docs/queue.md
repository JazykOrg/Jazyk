# Queue

The Queue is the job queue in the [Backend](backend.md). The
[API Server](api-server.md) enqueues a job whenever work should not hold up a
response. Worker processes of the Queue pull jobs and run them.

## Jobs

The Queue runs the following kinds of job:

- Email jobs, which render an email with the [Email Templates](api-server.md#rendering)
  and hand it to the mail provider.
- Export jobs, which run the [CSV Exporter](api-server.md#rendering) on a Project.
- The nightly cleanup job, which deletes expired rows from the
  [Database](database.md).
- Invoice jobs, which create the [Invoices](billing.md#invoice) of a billing period.

## Behavior

- The Queue stores every job in the Database table `jobs` with its kind, its payload,
  and its state.
- A job is in one of the states `queued`, `running`, `done`, or `failed`.
- A worker picks a `queued` job, marks it `running`, and marks it `done` when the job
  returns.
- When a job raises an error, the worker marks it `queued` again with a delay of 1
  minute, then 10 minutes, then 1 hour.
- After the third error the worker marks the job `failed` and reports it to
  [Monitoring](monitoring.md).
- A job runs for at most 15 minutes; a job still running after that is marked `failed`.
- The Queue runs at least two worker processes.
