# Platform

The Cache holds recently read records. The Queue carries work between services. The
Scheduler runs periodic jobs. Metrics counts what the services do.

## Rules

The Cache shall expire a record after ten minutes. The Scheduler shall run the nightly
job through the Queue. Metrics shall count every job the Scheduler runs. When the
Queue is full, the Scheduler shall wait instead of dropping a job.
