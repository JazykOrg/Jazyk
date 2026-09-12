use product::shipment::{Order, Shipment, ShipmentStatus};

#[test]
// req:shipping-4 [req_shipping_4_0cab498b]: If a Shipment cannot be delivered after two attempts, Orderly returns the Shipment to the warehouse.
fn req_shipping_4_0cab498b_two_failed_delivery_attempts_return_shipment_to_warehouse() {
    let order = Order { id: 404 };
    let mut shipment = Shipment::create(&order);

    shipment.record_attempt();
    assert_eq!(
        shipment.delivery_attempts, 1,
        "the first failed delivery attempt must be recorded"
    );
    assert_ne!(
        shipment.status,
        ShipmentStatus::ReturnedToWarehouse,
        "Orderly must not return the Shipment to the warehouse before two failed attempts"
    );

    shipment.record_attempt();
    assert_eq!(
        shipment.delivery_attempts, 2,
        "the second failed delivery attempt must be recorded"
    );

    let returned = shipment.handle_failed_delivery();

    assert!(
        returned,
        "after two failed delivery attempts, Orderly must perform the return-to-warehouse action"
    );
    assert_eq!(
        shipment.status,
        ShipmentStatus::ReturnedToWarehouse,
        "after two failed delivery attempts, the Shipment must be returned to the warehouse"
    );
}
