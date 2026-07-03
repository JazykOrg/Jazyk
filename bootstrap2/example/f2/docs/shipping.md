# Shipping

A Shipment delivers a packed [Order](./orders.md) to its buyer.

## Dispatch

When an Order is paid and every Product in it is in stock, the system shall create a
Shipment for it. When a Shipment leaves the warehouse, the system shall send the buyer
a tracking link.

## Delivery

If a Shipment cannot be delivered after two attempts, then the system shall return it
to the warehouse and refund the buyer.
