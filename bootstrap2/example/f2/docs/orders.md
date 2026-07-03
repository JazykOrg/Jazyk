# Order

An Order is a purchase a [Customer](./customer.md) places. An Order contains one or
more Products from the [Catalog](./catalog.md).

## Lifecycle

When a Customer submits an Order, the system shall reserve the Stock for each Product
in it. An Order shall be paid within 14 days of placement, otherwise the system shall
cancel it.

## Contents

An Order shall list each Product with its quantity and the price at the time of
placement.
