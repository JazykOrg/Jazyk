#[test]
fn req_system_4_4e85ad93() {
    // req:system-4 [req_system_4_4e85ad93]: A Customer pays through Payment in Orderly.
    // quote: A Customer browses the [Catalog](./catalog.md),
    // places an [Order](./orders.md), pays through [Payment](./payment.md), and receives a
    // [Shipment](./shipping.md).
    let customer_source = include_str!("../src/customer.rs");
    let payment_source = include_str!("../src/payment.rs");

    let uncommented_customer: String = customer_source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        payment_source.contains("struct Payment"),
        "Orderly must define the Payment component used for Customer payment"
    );
    assert!(
        uncommented_customer.contains("Payment")
            || uncommented_customer.contains("payment::")
            || uncommented_customer.contains("initialize_payment")
            || uncommented_customer.contains("confirm_payment"),
        "Customer behavior must pay through the Payment component, not only track paid/unpaid state"
    );
}
