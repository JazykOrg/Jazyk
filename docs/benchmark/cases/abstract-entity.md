# abstract-entity

Grades the fan-out variant of
[`abstract-entity`](../../compiler/goals/abstract-entity.md#the-fan-out-variant), in two
cases. The goal derives from the seeded fixture: the seeding commit counts the
parentless entities of the scope, writes `threshold-crossed` on `scope:public`, and
the board derives `g:abstract-entity:scope:public` with its coupling candidates and
member hints ([derived goals](../cases.md#derived-goals)).

## Twelve children from three documents

Twelve parentless entities come from `orders.md`, `shipping.md`, and `billing.md`. The
documents are the partition: the orders entities and the shipping entities each become
one grouping derived from exactly its members. `billing.md` is headed `Billing` and
states a `Billing` entity beside its invoice, payment, and refund: that stated entity
is the level's node for the document's entities (the `namesake` hint), so they move
under it and no twin named like it is minted. The level ends at three. See
[naming](../../compiler/concepts/levels.md#naming).

```yaml
name: abstract-entity
description: Group a top level of twelve children along its three documents, the stated namesake taking its own document's entities.
tier: structure
par:
  rounds: 5
goal:
  kind: abstract-entity
  target: scope:public
given:
  docs:
    docs/orders.md: |
      # Orders

      The orders area covers what a customer buys.

      ## Cart

      The cart holds the items a customer selected.

      ## Pricing

      Pricing computes the total of a cart.

      ## Checkout

      The checkout turns a priced cart into an order.

      ## Discount

      A discount lowers the pricing of a cart.
    docs/shipping.md: |
      # Shipping

      The shipping area moves orders to customers.

      ## Shipment

      A shipment carries an order to the customer.

      ## Carrier

      A carrier transports a shipment.

      ## Tracking

      Tracking follows a shipment through its carrier.

      ## Label

      A label is printed for each shipment.
    docs/billing.md: |
      # Billing

      Billing settles what a customer owes.

      ## Invoice

      An invoice is issued by billing for each order.

      ## Payment

      A payment settles an invoice.

      ## Refund

      A refund reverses a payment.
  graph:
    entities:
      ent:cart:
        name: Cart
        stereotype: component
        definition: The items a customer selected.
        mentions:
          - section: 'docs/orders.md#/orders/cart'
            quote: The cart holds the items a customer selected.
      ent:pricing:
        name: Pricing
        stereotype: component
        definition: Computes the total of a cart.
        mentions:
          - section: 'docs/orders.md#/orders/pricing'
            quote: Pricing computes the total of a cart.
      ent:checkout:
        name: Checkout
        stereotype: component
        definition: Turns a priced cart into an order.
        mentions:
          - section: 'docs/orders.md#/orders/checkout'
            quote: The checkout turns a priced cart into an order.
      ent:discount:
        name: Discount
        stereotype: component
        definition: Lowers the pricing of a cart.
        mentions:
          - section: 'docs/orders.md#/orders/discount'
            quote: A discount lowers the pricing of a cart.
      ent:shipment:
        name: Shipment
        stereotype: component
        definition: Carries an order to the customer.
        mentions:
          - section: 'docs/shipping.md#/shipping/shipment'
            quote: A shipment carries an order to the customer.
      ent:carrier:
        name: Carrier
        stereotype: actor
        definition: Transports a shipment.
        mentions:
          - section: 'docs/shipping.md#/shipping/carrier'
            quote: A carrier transports a shipment.
      ent:tracking:
        name: Tracking
        stereotype: component
        definition: Follows a shipment through its carrier.
        mentions:
          - section: 'docs/shipping.md#/shipping/tracking'
            quote: Tracking follows a shipment through its carrier.
      ent:label:
        name: Label
        stereotype: component
        definition: Printed for each shipment.
        mentions:
          - section: 'docs/shipping.md#/shipping/label'
            quote: A label is printed for each shipment.
      ent:billing:
        name: Billing
        stereotype: component
        definition: Settles what a customer owes.
        mentions:
          - section: 'docs/billing.md#/billing'
            quote: Billing settles what a customer owes.
      ent:invoice:
        name: Invoice
        stereotype: component
        definition: Issued by billing for each order.
        mentions:
          - section: 'docs/billing.md#/billing/invoice'
            quote: An invoice is issued by billing for each order.
      ent:payment:
        name: Payment
        stereotype: component
        definition: Settles an invoice.
        mentions:
          - section: 'docs/billing.md#/billing/payment'
            quote: A payment settles an invoice.
      ent:refund:
        name: Refund
        stereotype: component
        definition: Reverses a payment.
        mentions:
          - section: 'docs/billing.md#/billing/refund'
            quote: A refund reverses a payment.
    requirements:
      req:orders-1:
        statement: The cart holds the items a customer selected.
        entities: [ent:cart]
        source:
          section: 'docs/orders.md#/orders/cart'
          quote: The cart holds the items a customer selected.
      req:orders-2:
        statement: Pricing computes the total of a cart.
        entities: [ent:pricing, ent:cart]
        edges:
          - {a: ent:pricing, b: ent:cart, type: dependency}
        source:
          section: 'docs/orders.md#/orders/pricing'
          quote: Pricing computes the total of a cart.
      req:orders-3:
        statement: The checkout turns a priced cart into an order.
        entities: [ent:checkout, ent:cart, ent:pricing]
        edges:
          - {a: ent:checkout, b: ent:cart, type: dependency}
          - {a: ent:checkout, b: ent:pricing, type: dependency}
        source:
          section: 'docs/orders.md#/orders/checkout'
          quote: The checkout turns a priced cart into an order.
      req:orders-4:
        statement: A discount lowers the pricing of a cart.
        entities: [ent:discount, ent:pricing, ent:cart]
        edges:
          - {a: ent:discount, b: ent:pricing, type: dependency}
        source:
          section: 'docs/orders.md#/orders/discount'
          quote: A discount lowers the pricing of a cart.
      req:shipping-1:
        statement: A shipment carries an order to the customer.
        entities: [ent:shipment]
        source:
          section: 'docs/shipping.md#/shipping/shipment'
          quote: A shipment carries an order to the customer.
      req:shipping-2:
        statement: A carrier transports a shipment.
        entities: [ent:carrier, ent:shipment]
        edges:
          - {a: ent:carrier, b: ent:shipment, type: dependency}
        source:
          section: 'docs/shipping.md#/shipping/carrier'
          quote: A carrier transports a shipment.
      req:shipping-3:
        statement: Tracking follows a shipment through its carrier.
        entities: [ent:tracking, ent:shipment, ent:carrier]
        edges:
          - {a: ent:tracking, b: ent:shipment, type: dependency}
          - {a: ent:tracking, b: ent:carrier, type: dependency}
        source:
          section: 'docs/shipping.md#/shipping/tracking'
          quote: Tracking follows a shipment through its carrier.
      req:shipping-4:
        statement: A label is printed for each shipment.
        entities: [ent:label, ent:shipment]
        edges:
          - {a: ent:label, b: ent:shipment, type: dependency}
        source:
          section: 'docs/shipping.md#/shipping/label'
          quote: A label is printed for each shipment.
      req:billing-1:
        statement: Billing settles what a customer owes.
        entities: [ent:billing]
        source:
          section: 'docs/billing.md#/billing'
          quote: Billing settles what a customer owes.
      req:billing-2:
        statement: An invoice is issued by billing for each order.
        entities: [ent:invoice, ent:billing]
        edges:
          - {a: ent:billing, b: ent:invoice, type: dependency}
        source:
          section: 'docs/billing.md#/billing/invoice'
          quote: An invoice is issued by billing for each order.
      req:billing-3:
        statement: A payment settles an invoice.
        entities: [ent:payment, ent:invoice]
        edges:
          - {a: ent:payment, b: ent:invoice, type: dependency}
        source:
          section: 'docs/billing.md#/billing/payment'
          quote: A payment settles an invoice.
      req:billing-4:
        statement: A refund reverses a payment.
        entities: [ent:refund, ent:payment]
        edges:
          - {a: ent:refund, b: ent:payment, type: dependency}
        source:
          section: 'docs/billing.md#/billing/refund'
          quote: A refund reverses a payment.
assert:
  - childCount:
      parent: scope:public
      max: 9
  - groupingOf:
      members: [ent:cart, ent:pricing, ent:checkout, ent:discount]
  - groupingOf:
      members: [ent:shipment, ent:carrier, ent:tracking, ent:label]
  - parentIs:
      entity: ent:invoice
      parent: ent:billing
  - parentIs:
      entity: ent:payment
      parent: ent:billing
  - parentIs:
      entity: ent:refund
      parent: ent:billing
  - entityNameCount:
      namePattern: billing
      max: 1
  - nodeExists:
      id: ent:billing
  - entityCount:
      max: 14
```

## The namesake collision

`checkout.md` is headed `Checkout` and states the checkout as a process beside the
cart, the address form, the payment step, the order review, and the confirmation it
describes. Two more documents bring seven peers, thirteen in all. The stated process
is the level's node for its document's entities: they move under it with
`update_entity` `parent`, and no new entity named like it appears. The move alone
brings the level to eight, so the gate is satisfied by the namesake rule and the
checks grade exactly that rule.

```yaml
name: abstract-entity-namesake
description: A stated process named like its document takes that document's entities as children instead of gaining a twin grouping.
tier: structure
par:
  rounds: 5
goal:
  kind: abstract-entity
  target: scope:public
given:
  docs:
    docs/checkout.md: |
      # Checkout

      The checkout is the process that turns a cart into a paid order.

      ## Cart

      The cart lists what the customer is buying.

      ## Address form

      The address form collects the shipping address.

      ## Payment step

      The payment step charges the customer's card.

      ## Order review

      The order review shows the cart, the address, and the total before payment.

      ## Confirmation

      The confirmation shows the order number after payment.
    docs/catalog.md: |
      # Product catalog

      The catalog describes what the shop sells.

      ## Product

      A product has a name and a price.

      ## Category

      A category groups products.

      ## Search index

      The search index answers product searches.

      ## Price list

      The price list holds the current price of every product.
    docs/accounts.md: |
      # Accounts

      Accounts identify the shop's customers.

      ## Account

      An account identifies a customer.

      ## Login

      The login checks an account's password.

      ## Password reset

      The password reset emails the account a reset link.
  graph:
    entities:
      ent:checkout:
        name: Checkout
        stereotype: process
        definition: The process that turns a cart into a paid order.
        mentions:
          - section: 'docs/checkout.md#/checkout'
            quote: The checkout is the process that turns a cart into a paid order.
      ent:cart:
        name: Cart
        definition: Lists what the customer is buying.
        mentions:
          - section: 'docs/checkout.md#/checkout/cart'
            quote: The cart lists what the customer is buying.
      ent:address-form:
        name: Address Form
        definition: Collects the shipping address.
        mentions:
          - section: 'docs/checkout.md#/checkout/address-form'
            quote: The address form collects the shipping address.
      ent:payment-step:
        name: Payment Step
        definition: Charges the customer's card.
        mentions:
          - section: 'docs/checkout.md#/checkout/payment-step'
            quote: The payment step charges the customer's card.
      ent:order-review:
        name: Order Review
        definition: Shows the cart, the address, and the total before payment.
        mentions:
          - section: 'docs/checkout.md#/checkout/order-review'
            quote: The order review shows the cart, the address, and the total before payment.
      ent:confirmation:
        name: Confirmation
        definition: Shows the order number after payment.
        mentions:
          - section: 'docs/checkout.md#/checkout/confirmation'
            quote: The confirmation shows the order number after payment.
      ent:product:
        name: Product
        definition: Has a name and a price.
        mentions:
          - section: 'docs/catalog.md#/product-catalog/product'
            quote: A product has a name and a price.
      ent:category:
        name: Category
        definition: Groups products.
        mentions:
          - section: 'docs/catalog.md#/product-catalog/category'
            quote: A category groups products.
      ent:search-index:
        name: Search Index
        definition: Answers product searches.
        mentions:
          - section: 'docs/catalog.md#/product-catalog/search-index'
            quote: The search index answers product searches.
      ent:price-list:
        name: Price List
        definition: Holds the current price of every product.
        mentions:
          - section: 'docs/catalog.md#/product-catalog/price-list'
            quote: The price list holds the current price of every product.
      ent:account:
        name: Account
        definition: Identifies a customer.
        mentions:
          - section: 'docs/accounts.md#/accounts/account'
            quote: An account identifies a customer.
      ent:login:
        name: Login
        definition: Checks an account's password.
        mentions:
          - section: 'docs/accounts.md#/accounts/login'
            quote: The login checks an account's password.
      ent:password-reset:
        name: Password Reset
        definition: Emails the account a reset link.
        mentions:
          - section: 'docs/accounts.md#/accounts/password-reset'
            quote: The password reset emails the account a reset link.
    requirements:
      req:checkout-1:
        statement: The checkout is the process that turns a cart into a paid order.
        entities: [ent:checkout, ent:cart]
        edges:
          - {a: ent:checkout, b: ent:cart, type: dependency}
        source:
          section: 'docs/checkout.md#/checkout'
          quote: The checkout is the process that turns a cart into a paid order.
      req:checkout-2:
        statement: The cart lists what the customer is buying.
        entities: [ent:cart]
        source:
          section: 'docs/checkout.md#/checkout/cart'
          quote: The cart lists what the customer is buying.
      req:checkout-3:
        statement: The address form collects the shipping address.
        entities: [ent:address-form]
        source:
          section: 'docs/checkout.md#/checkout/address-form'
          quote: The address form collects the shipping address.
      req:checkout-4:
        statement: The payment step charges the customer's card.
        entities: [ent:payment-step]
        source:
          section: 'docs/checkout.md#/checkout/payment-step'
          quote: The payment step charges the customer's card.
      req:checkout-5:
        statement: The order review shows the cart, the address, and the total before payment.
        entities: [ent:order-review, ent:cart, ent:address-form]
        edges:
          - {a: ent:order-review, b: ent:cart, type: dependency}
          - {a: ent:order-review, b: ent:address-form, type: dependency}
        source:
          section: 'docs/checkout.md#/checkout/order-review'
          quote: The order review shows the cart, the address, and the total before payment.
      req:checkout-6:
        statement: The confirmation shows the order number after payment.
        entities: [ent:confirmation]
        source:
          section: 'docs/checkout.md#/checkout/confirmation'
          quote: The confirmation shows the order number after payment.
      req:catalog-1:
        statement: A product has a name and a price.
        entities: [ent:product]
        source:
          section: 'docs/catalog.md#/product-catalog/product'
          quote: A product has a name and a price.
      req:catalog-2:
        statement: A category groups products.
        entities: [ent:category, ent:product]
        edges:
          - {a: ent:category, b: ent:product, type: aggregation}
        source:
          section: 'docs/catalog.md#/product-catalog/category'
          quote: A category groups products.
      req:catalog-3:
        statement: The search index answers product searches.
        entities: [ent:search-index, ent:product]
        edges:
          - {a: ent:search-index, b: ent:product, type: dependency}
        source:
          section: 'docs/catalog.md#/product-catalog/search-index'
          quote: The search index answers product searches.
      req:catalog-4:
        statement: The price list holds the current price of every product.
        entities: [ent:price-list, ent:product]
        edges:
          - {a: ent:price-list, b: ent:product, type: dependency}
        source:
          section: 'docs/catalog.md#/product-catalog/price-list'
          quote: The price list holds the current price of every product.
      req:accounts-1:
        statement: An account identifies a customer.
        entities: [ent:account]
        source:
          section: 'docs/accounts.md#/accounts/account'
          quote: An account identifies a customer.
      req:accounts-2:
        statement: The login checks an account's password.
        entities: [ent:login, ent:account]
        edges:
          - {a: ent:login, b: ent:account, type: dependency}
        source:
          section: 'docs/accounts.md#/accounts/login'
          quote: The login checks an account's password.
      req:accounts-3:
        statement: The password reset emails the account a reset link.
        entities: [ent:password-reset, ent:account]
        edges:
          - {a: ent:password-reset, b: ent:account, type: dependency}
        source:
          section: 'docs/accounts.md#/accounts/password-reset'
          quote: The password reset emails the account a reset link.
assert:
  - parentIs:
      entity: ent:cart
      parent: ent:checkout
  - parentIs:
      entity: ent:address-form
      parent: ent:checkout
  - parentIs:
      entity: ent:payment-step
      parent: ent:checkout
  - parentIs:
      entity: ent:order-review
      parent: ent:checkout
  - parentIs:
      entity: ent:confirmation
      parent: ent:checkout
  - entityNameCount:
      namePattern: checkout
      max: 1
  - nodeExists:
      id: ent:checkout
  - childCount:
      parent: scope:public
      max: 9
  - entityCount:
      max: 15
```
