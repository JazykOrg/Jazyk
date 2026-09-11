# Orders

An Order is a customer's request to buy one or more items. An Invoice is the bill an
Order produces. A Payment settles an Invoice. A Refund reverses a Payment.

## Rules

Every Order shall produce exactly one Invoice when it is confirmed. A Payment shall
settle the whole Invoice it names. A Refund shall never exceed the Payment it reverses.
When a Refund is issued, the system shall mark the Order as refunded.
