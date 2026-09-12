#[test]
fn req_system_5_2cf777e3() {
    // req:system-5 [req_system_5_2cf777e3]: A Customer receives a Shipment in Orderly.
    // quote: A Customer browses the [Catalog](./catalog.md),
    // places an [Order](./orders.md), pays through [Payment](./payment.md), and receives a
    // [Shipment](./shipping.md).
    let customer_source = include_str!("../src/customer.rs");
    let shipment_source = include_str!("../src/shipment.rs");

    let uncommented_customer = customer_source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let uncommented_shipment = shipment_source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();

    assert!(
        shipment_source.contains("struct Shipment"),
        "Orderly must define the Shipment component that a Customer receives"
    );
    assert!(
        (uncommented_customer.contains("shipment") && uncommented_customer.contains("receiv"))
            || (uncommented_shipment.contains("customer") && uncommented_shipment.contains("receiv")),
        "the artifact must model a Customer receiving a Shipment, not merely creating or shipping one"
    );
}
