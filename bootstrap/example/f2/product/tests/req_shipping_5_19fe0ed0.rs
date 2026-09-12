use product::shipment::{Order, Shipment, ShipmentStatus};

#[test]
// req:shipping-5 [req_shipping_5_19fe0ed0]: If a Shipment cannot be delivered after two attempts, Orderly refunds the buyer.
fn req_shipping_5_19fe0ed0_two_failed_delivery_attempts_refund_buyer() {
    let order = Order { id: 505 };
    let mut shipment = Shipment::create(&order);

    shipment.record_attempt();
    assert_eq!(
        shipment.delivery_attempts, 1,
        "the first failed delivery attempt must be recorded"
    );
    assert!(
        !shipment.buyer_refunded,
        "Orderly must not refund the buyer before the second failed delivery attempt is handled"
    );

    shipment.record_attempt();
    assert_eq!(
        shipment.delivery_attempts, 2,
        "the second failed delivery attempt must be recorded"
    );

    let refund_performed = shipment.handle_failed_delivery();

    assert_eq!(
        shipment.status,
        ShipmentStatus::ReturnedToWarehouse,
        "after two failed delivery attempts, the Shipment must be returned before refund completion is observable"
    );
    assert!(
        refund_performed,
        "after two failed delivery attempts, Orderly must perform the buyer refund"
    );
    assert!(
        shipment.buyer_refunded,
        "after two failed delivery attempts, the Shipment must record that the buyer was refunded"
    );
}
