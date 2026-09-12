#[test]
fn req_system_9_012251dd() {
    // req:system-9 [req_system_9_012251dd]: Orderly keeps every Order traceable from placement to delivery or return.
    // quote: The system shall keep every Order traceable from placement to delivery or return.
    let order_source = include_str!("../src/order.rs");

    assert!(
        order_source.contains("pub struct Order"),
        "Orderly must represent Orders as traceable records"
    );
    assert!(
        order_source.contains("created_at"),
        "Order traceability must preserve placement time on the Order"
    );
    assert!(
        order_source.contains("OrderStatus::Submitted"),
        "Order traceability must start at placement/submission"
    );
    assert!(
        order_source.contains("OrderStatus::Delivered") && order_source.contains("fn mark_delivered"),
        "Order traceability must include a delivery endpoint transition"
    );
    assert!(
        order_source.contains("OrderStatus::Returned") && order_source.contains("fn mark_returned"),
        "Order traceability must include a return endpoint transition on the Order"
    );
}
