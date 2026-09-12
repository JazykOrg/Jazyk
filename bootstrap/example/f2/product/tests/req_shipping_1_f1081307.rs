#[test]
fn req_shipping_1_f1081307_shipment_delivers_packed_order_to_buyer() {
    let shipment_source = include_str!("../src/shipment.rs");

    assert!(
        shipment_source.contains("Shipment"),
        "the deliverable must define a Shipment"
    );
    assert!(
        shipment_source.contains("Order") && shipment_source.contains("order_id"),
        "a Shipment must be tied to the Order it delivers"
    );
    assert!(
        shipment_source.to_lowercase().contains("packed"),
        "a Shipment must represent delivery of a packed Order"
    );
    assert!(
        shipment_source.to_lowercase().contains("buyer"),
        "a Shipment must represent delivery to the buyer"
    );
}
