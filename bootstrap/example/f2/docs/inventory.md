# Inventory

Inventory tracks the Stock of every Product in the warehouse.

## Stock rules

When a Product is picked for a Shipment, the system shall decrease its Stock by the
picked quantity. If the Stock of a Product reaches its reorder point, then the system
shall create a restock task.

## Examples

A typical day: 40 units of SKU-1042 arrive in the morning and the shelf count rises.
Note that whatever the day brings, the Stock count shall never go below zero.
