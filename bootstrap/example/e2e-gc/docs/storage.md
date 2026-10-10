# Data Layer

The Data Layer holds every store the services share.

## Stores

The Data Layer contains the User Store. The Data Layer contains the Product Store.
The Data Layer contains the Order Store. The Data Layer contains the Payment Store.
The Data Layer contains the Session Cache.

## Access

The Auth Service reads the User Store. The Catalog Service writes the Product Store.
The Search Service scans the Product Store. The Cart Service updates the Session Cache.
The Checkout Service writes the Order Store. The Pricing Service reads the Product Store.
The Promotion Service caches rules in the Session Cache. The Review Service appends to the Product Store.
The Billing Service writes the Payment Store. The Inventory Service updates the Product Store.
The Shipping Service reads the Order Store. The Notification Service reads the User Store.
The Analytics Service scans the Order Store. The Reporting Service aggregates the Payment Store.
The Support Service reads the User Store. The Fraud Service inspects the Payment Store.
The Billing Service appends every charge to the Audit Log. The Audit Log keeps one line per charge.
