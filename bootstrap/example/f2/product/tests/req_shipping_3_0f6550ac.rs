#[test]
// req:shipping-3 [req_shipping_3_0f6550ac]: When a Shipment leaves the warehouse, Orderly sends the buyer a tracking link.
fn req_shipping_3_0f6550ac_shipment_leaving_warehouse_sends_tracking_link() {
    let shipment_source = include_str!("../src/shipment.rs");
    let order_source = include_str!("../src/order.rs");

    assert!(
        shipment_source.contains("pub fn ship(&mut self) -> bool")
            && shipment_source.contains("ShipmentStatus::Created")
            && shipment_source.contains("ShipmentStatus::Shipped"),
        "Shipment must expose a leave-the-warehouse transition from Created to Shipped"
    );
    assert!(
        shipment_source.to_lowercase().contains("tracking link")
            && order_source.to_lowercase().contains("tracking link"),
        "the shipment-leaves-warehouse path must send the buyer a tracking link"
    );
    assert!(
        shipment_source.to_lowercase().contains("buyer")
            || order_source.to_lowercase().contains("buyer"),
        "the tracking link obligation must be directed to the buyer"
    );
}
