# Billing

This document describes the classes behind payment: Plan, Subscription, and Invoice.
The [API Server](api-server.md) persists all three in the [Database](database.md); the
[Billing Handler](api-server.md#request-handling) writes them. Card charges go through
an outside payment provider, and the Billing Handler receives the provider's webhooks.

## Plan

A Plan is a priced tier of the product. A Plan has a name, a monthly price, a Member
limit, and a Project limit. There are three Plans: `free` with 3 Members and 2
Projects at no charge, `team` with 25 Members and unlimited Projects at 8 dollars per
Member per month, and `business` with unlimited Members and Projects at 15 dollars per
Member per month. The API Server caches the list of Plans in the [Cache](cache.md) for
1 hour.

## Subscription

A Subscription is an [Organization](identity.md#organization)'s standing order for a
Plan. A Subscription has an Organization, a Plan, a billing period start, and a status.
Every Organization holds exactly one Subscription, created on the `free` Plan when the
Organization is created. The status of a Subscription is `active` or `cancelled`. A
Subscription is created in the status `active`. An owner changes the Plan of the Subscription from the Organization settings page, and the
change takes effect at the next billing period. When an owner cancels the Subscription,
it becomes `cancelled` and the Organization falls back to the `free` Plan at the end of
the paid period. The Organization Handler refuses to add a Member beyond the Member
limit of the Subscription's Plan.

## Invoice

An Invoice is the amount due for one billing period of a Subscription. An Invoice has a
Subscription, a period, an amount, a status, and an optional payment date. The status
of an Invoice is `open`, `paid`, or `overdue`. An Invoice is created in the status
`open`. The amount of an Invoice is the Plan's monthly price times the number of
Members on the first day of the period. An Invoice on the `free` Plan has an amount of
zero and is marked `paid` at creation. An Invoice keeps a refund note when
[Support](support.md) refunds it.

## Paying an invoice

The product charges each Organization once per billing period:

1. On the first day of each billing period, an invoice job on the [Queue](queue.md)
   creates an Invoice for every `active` Subscription.
2. The Billing Handler enqueues an invoice email job on the Queue for every Invoice
   with an amount above zero.
3. The [Email Templates](api-server.md#rendering) render the invoice email with the
   amount due and a link to the billing page.
4. The User opens the billing page in the [Frontend](frontend.md) and enters a card.
5. The Frontend sends the card token to the API Server.
6. The Billing Handler charges the card through the payment provider and marks the
   Invoice `paid` with the payment date.
7. The Billing Handler writes an AuditEntry for the payment.

When the charge fails, the Billing Handler marks the Invoice `overdue` and creates a
Notification for every owner of the Organization. When an Invoice stays `overdue` for
14 days, the Billing Handler cancels the Subscription. A User pays an `overdue` Invoice
the same way, and it becomes `paid`.
