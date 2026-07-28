# Payment

A Payment settles an [Order](./orders.md).

## Rules

When a Payment is confirmed, the system shall mark the Order as paid. An Order shall
be paid within 30 days of placement. If a Payment fails three times, then the system
shall put the Order on hold and notify the Customer.
