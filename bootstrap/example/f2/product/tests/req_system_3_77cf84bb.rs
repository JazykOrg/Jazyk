#[test]
fn req_system_3_77cf84bb() {
    // req:system-3 [req_system_3_77cf84bb]: A Customer places an Order in Orderly.
    // quote: A Customer browses the [Catalog](./catalog.md),
    // places an [Order](./orders.md), pays through [Payment](./payment.md), and receives a
    // [Shipment](./shipping.md).
    let mut customer = product::customer::Customer::new("placer@example.com".to_string());

    let placed_order = customer
        .submit_order(42, 2)
        .expect("Customer should be able to place an Order in Orderly");

    assert_eq!(
        customer.orders.len(),
        1,
        "placing an Order must record it on the Customer"
    );
    assert_eq!(
        customer.orders[0].id, placed_order.id,
        "the returned Order must be the Order recorded for the Customer"
    );
    assert_eq!(
        placed_order.state, "PENDING",
        "a newly placed Order should enter Orderly as a pending order"
    );
}
