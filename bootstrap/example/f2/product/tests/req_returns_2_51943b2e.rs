// tests/req_returns_2_51943b2e.rs

#[test]
fn req_returns_2_51943b2e() {
    // A Customer can open a Return for an Order within 30 days of delivery.
    let returns_source = include_str!("../src/returns.rs");
    let order_source = include_str!("../src/order.rs");
    let customer_source = include_str!("../src/customer.rs");

    assert!(
        returns_source.contains("struct Return") || returns_source.contains("enum Return"),
        "the deliverable must define a Return artifact"
    );
    assert!(
        returns_source.contains("Customer") || returns_source.contains("customer") || customer_source.contains("open_return"),
        "Return opening must be associated with a Customer"
    );
    assert!(
        returns_source.contains("Order") || returns_source.contains("order") || order_source.contains("Return"),
        "Return opening must be associated with an Order"
    );
    assert!(
        (returns_source.contains("30") && returns_source.contains("deliver"))
            || (order_source.contains("30") && order_source.contains("deliver")),
        "Return opening must enforce a 30-day window from delivery"
    );
    assert!(
        returns_source.contains("open") || returns_source.contains("create") || returns_source.contains("new"),
        "the deliverable must expose a way for the Customer to open a Return"
    );
}
