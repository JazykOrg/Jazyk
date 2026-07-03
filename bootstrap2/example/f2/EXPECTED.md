# F2 expected outcomes (hand-labeled, not part of the docs glob)

Entities (~10): Customer, Order, Payment, Shipment, Product, Catalog, Stock (or
Inventory), Return, Admin CLI, Email (borderline), Orderly (borderline). buyer must
NOT survive as a separate entity (see traps).

Requirements: roughly 18 to 22 shall-statements across the docs.

Planted traps:
1. Cross-doc identity: `Order` is defined in orders.md and used in payment.md,
   shipping.md, returns.md, system.md, admin.md. Must be ONE entity with mentions in
   at least 3 documents.
2. Duplicate pair: shipping.md consistently says "buyer" for what customer.md calls
   Customer. Expect either reuse of ent:customer at extraction time, or a wave-2
   merge/duplicate-entity diagnostic.
3. Contradiction: orders.md says an Order shall be paid within 14 days; payment.md
   says within 30 days. Expect exactly one contradiction diagnostic on ent:order.
4. Non-normative trap: inventory.md "Examples" section hides a real rule ("the Stock
   count shall never go below zero"). Marking it non-normative must trigger
   suspicious-non-normative; extracting the rule and marking covered is also correct.
5. Junk bait: admin.md is full of flags (--port, --verbose), paths
   (/etc/orderly/config.toml), and commands. None of these may become entities.
   `Admin CLI` itself is a legitimate entity.
6. Genuinely non-normative: glossary.md and roadmap.md state no requirements and
   should be marked non-normative without a suspicious-non-normative finding.
