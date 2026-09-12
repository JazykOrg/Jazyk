use product::order::{Order, OrderItem, OrderStatus};

#[test]
// req:shipping-2 [req_shipping_2_58de09dc]: When an Order is paid and every Product in it is in stock, Orderly creates a Shipment for the Order.
fn req_shipping_2_58de09dc_paid_in_stock_order_gets_shipment() {
    let items = vec![OrderItem {
        product_id: "P-in-stock".to_string(),
        quantity: 2,
        placement_price: 10.0,
    }];
    let mut order = Order::new(
        "O-paid-in-stock".to_string(),
        "C-shipping".to_string(),
        items,
    );

    order.mark_as_paid();
    assert!(
        order.can_be_shipped(),
        "a paid order whose requested product quantities are available must be eligible for shipment"
    );

    order.create_shipment("S-created-for-order".to_string());

    assert_eq!(
        order.status,
        OrderStatus::Shipped,
        "shipment creation should move the paid, in-stock order into shipment"
    );
    assert_eq!(
        order.shipment_id.as_deref(),
        Some("S-created-for-order"),
        "Orderly must create and associate a Shipment with the paid, in-stock order"
    );
}
