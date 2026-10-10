# Data Layer

The Data Layer holds every store the services share.

## Stores

The Data Layer contains the User Store. The Data Layer contains the Product Store.
The Data Layer contains the Order Store. The Data Layer contains the Payment Store.
The Data Layer contains the Session Cache.

## Access

The Auth service reads the User Store. The Catalog service writes the Product Store.
The Search service scans the Product Store. The Cart service updates the Session Cache.
The Checkout service writes the Order Store. The Pricing service reads the Product Store.
The Promotion service caches rules in the Session Cache. The Review service appends to the Product Store.
The Billing service writes the Payment Store. The Inventory service updates the Product Store.
The Shipping service reads the Order Store. The Notification service reads the User Store.
The Analytics service scans the Order Store. The Reporting service aggregates the Payment Store.
The Support service reads the User Store. The Fraud service inspects the Payment Store.
The Billing service appends every charge to the Audit Log. The Audit Log keeps one line per charge.
