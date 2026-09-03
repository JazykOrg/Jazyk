# Checkout

A Cart holds the items a customer intends to buy. A Line Item is one product and
quantity in a Cart. A Coupon lowers the price of a Cart. A Discount is the amount a
Coupon takes off. A Tax is the amount the shop adds to a Cart for the state. An
Address is where a customer wants an Order delivered. A Receipt is the record a
customer keeps after a Payment. A Checkout turns a Cart into an Order. A Gift Card is a prepaid balance a customer
spends at Checkout. A Promotion is a shop-wide price rule active for a period. A
Wishlist holds the items a customer may buy later. A Loyalty Point is earned per
Order and spent at Checkout. A Payment Method is the card or balance a Checkout
charges.

## Rules

- A Checkout shall refuse an empty Cart.
- Every Line Item shall name exactly one product.
- A Coupon shall apply to a Cart at most once.
- The Receipt shall list every Line Item of the Order.
- A Checkout shall require an Address before it creates an Order.
- A Gift Card shall never be charged beyond its balance.
- A Promotion shall apply to every Cart while it is active.
- A Checkout shall spend Loyalty Points before it charges a Payment Method.
