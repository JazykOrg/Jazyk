#[path = "../src/orderly.rs"]
mod orderly;

use orderly::*;

fn sample_orderly() -> Orderly {
    Orderly::online_shop_system(
        Catalog::new(vec![
            Product::new("sku-1", "Tea", true),
            Product::new("sku-2", "Coffee", false),
        ]),
        Inventory::new(vec![StockLine { product_id: "sku-1".to_string(), quantity: 3, reserved: 0 }]),
    )
}

fn sample_customer() -> Customer {
    Customer::person_or_organization_with_account("cust-1", CustomerKind::Person, "buyer@example.com")
}

fn sample_order() -> Order {
    Order::placed_in_orderly("ord-1", "cust-1", vec![OrderLine { product_id: "sku-1".to_string(), quantity: 1 }])
}

// req:req:admin-1 hash:e8928a4d
#[test]
fn req_admin_1_e8928a4d() {
    let orderly = sample_orderly();
    assert_eq!(orderly.admin_cli_is_operator_tool(), "Admin CLI");
}

// req:req:admin-2 hash:1683a24d
#[test]
fn req_admin_2_1683a24d() {
    assert_eq!(Orderly::start_command(), "orderly serve --port 8080 --config /etc/orderly/config.toml");
}

// req:req:catalog-1 hash:2dd1e729
#[test]
fn req_catalog_1_2dd1e729() {
    let orderly = sample_orderly();
    let listed = orderly.catalog.lists_every_product_orderly_sells();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|product| product.name == "Tea"));
    assert!(listed.iter().any(|product| product.name == "Coffee"));
}

// req:req:catalog-4 hash:72d3d04b
#[test]
fn req_catalog_4_72d3d04b() {
    let orderly = sample_orderly();
    let visible = orderly.catalog.visible_products();
    assert_eq!(visible.len(), 1);
    assert!(visible.iter().all(|product| product.in_stock));
}

// req:req:customer-1 hash:f180c28a
#[test]
fn req_customer_1_f180c28a() {
    let customer = sample_customer();
    assert_eq!(customer.kind, CustomerKind::Person);
    assert!(customer.holds_orderly_account);
}

// req:req:customer-5 hash:91839be5
#[test]
fn req_customer_5_91839be5() {
    let mut customer = sample_customer();
    customer.notify_by_email_for_order_state_change("ord-1", OrderState::Paid);
    assert!(customer.notifications.iter().any(|message| message.contains("Email") && message.contains("ord-1") && message.contains("Paid")));
}

// req:req:orders-3 hash:ebd76a10
#[test]
fn req_orders_3_ebd76a10() {
    let mut orderly = sample_orderly();
    let mut order = sample_order();
    assert!(order.submit_and_reserve_stock(&mut orderly.inventory));
    let stock = orderly.inventory.stock.iter().find(|stock| stock.product_id == "sku-1").unwrap();
    assert_eq!(stock.quantity, 2);
    assert_eq!(stock.reserved, 1);
}

// req:req:orders-4 hash:29f2c263
#[test]
fn req_orders_4_29f2c263() {
    let mut order = sample_order();
    order.days_since_placement = 22;
    assert!(order.cancel_if_not_paid_within_21_days());
    assert_eq!(order.state, OrderState::Canceled);
}

// req:req:payment-2 hash:2f52f7b4
#[test]
fn req_payment_2_2f52f7b4() {
    let mut order = sample_order();
    let mut payment = Payment::new("pay-1", "ord-1");
    payment.confirm();
    assert!(order.mark_paid_when_payment_confirmed(&payment));
    assert_eq!(order.state, OrderState::Paid);
}

// req:req:payment-4 hash:6f3f1877
#[test]
fn req_payment_4_6f3f1877() {
    let mut order = sample_order();
    let mut payment = Payment::new("pay-1", "ord-1");
    payment.fail();
    payment.fail();
    payment.fail();
    assert!(order.put_on_hold_after_three_payment_failures(&payment));
    assert_eq!(order.state, OrderState::OnHold);
}

// req:req:payment-5 hash:2793ad62
#[test]
fn req_payment_5_2793ad62() {
    let mut customer = sample_customer();
    let mut payment = Payment::new("pay-1", "ord-1");
    payment.fail();
    payment.fail();
    payment.fail();
    assert!(notify_customer_after_three_payment_failures(&mut customer, &payment));
    assert!(customer.notifications.iter().any(|message| message.contains("Payment failed three times")));
}

// req:req:returns-3 hash:ebd60fa9
#[test]
fn req_returns_3_ebd60fa9() {
    let return_request = Return { id: "ret-1".to_string(), order_id: "ord-1".to_string(), received: true, inspected: true };
    let mut payment = Payment::new("pay-1", "ord-1");
    payment.confirm();
    assert!(refund_payment_when_return_received_and_inspected(&return_request, &mut payment));
    assert_eq!(payment.status, PaymentStatus::Refunded);
}

// req:req:returns-4 hash:264fa475
#[test]
fn req_returns_4_264fa475() {
    let return_request = Return { id: "ret-1".to_string(), order_id: "ord-1".to_string(), received: true, inspected: true };
    let mut inventory = Inventory::new(vec![StockLine { product_id: "sku-1".to_string(), quantity: 0, reserved: 0 }]);
    let line = OrderLine { product_id: "sku-1".to_string(), quantity: 2 };
    assert!(restore_stock_when_return_received_and_inspected(&return_request, &mut inventory, &line));
    assert_eq!(inventory.stock[0].quantity, 2);
}

// req:req:shipping-2 hash:58de09dc
#[test]
fn req_shipping_2_58de09dc() {
    let orderly = sample_orderly();
    let mut order = sample_order();
    order.state = OrderState::Paid;
    let shipment = order.create_shipment_when_paid_and_in_stock(&orderly.inventory, "ship-1");
    assert!(shipment.is_some());
    assert_eq!(shipment.unwrap().order_id, "ord-1");
}

// req:req:shipping-3 hash:0f6550ac
#[test]
fn req_shipping_3_0f6550ac() {
    let orderly = sample_orderly();
    let mut order = sample_order();
    order.state = OrderState::Paid;
    let mut shipment = order.create_shipment_when_paid_and_in_stock(&orderly.inventory, "ship-1").unwrap();
    let link = shipment.leave_warehouse_and_send_tracking_link("https://track.example/ship-1");
    assert_eq!(link, "https://track.example/ship-1");
    assert_eq!(shipment.state, ShipmentState::LeftWarehouse);
    assert_eq!(shipment.tracking_links.len(), 1);
}

// req:req:shipping-4 hash:0cab498b
#[test]
fn req_shipping_4_0cab498b() {
    let mut shipment = Shipment { id: "ship-1".to_string(), order_id: "ord-1".to_string(), buyer_id: "cust-1".to_string(), state: ShipmentState::LeftWarehouse, tracking_links: Vec::new(), delivery_attempts: 0, buyer_refunded: false };
    shipment.record_failed_delivery_attempt();
    shipment.record_failed_delivery_attempt();
    assert!(shipment.return_to_warehouse_after_two_attempts());
    assert_eq!(shipment.state, ShipmentState::ReturnedToWarehouse);
}

// req:req:shipping-5 hash:19fe0ed0
#[test]
fn req_shipping_5_19fe0ed0() {
    let mut shipment = Shipment { id: "ship-1".to_string(), order_id: "ord-1".to_string(), buyer_id: "cust-1".to_string(), state: ShipmentState::LeftWarehouse, tracking_links: Vec::new(), delivery_attempts: 2, buyer_refunded: false };
    assert!(shipment.refund_buyer_after_two_attempts());
    assert!(shipment.buyer_refunded);
}

// req:req:system-1 hash:db8c571c
#[test]
fn req_system_1_db8c571c() {
    let orderly = sample_orderly();
    assert_eq!(orderly.admin_cli_tool, "Admin CLI");
    assert_eq!(orderly.catalog.lists_every_product_orderly_sells().len(), 2);
}

// req:req:system-10 hash:5d5eb8e9
#[test]
fn req_system_10_5d5eb8e9() {
    let mut orderly = sample_orderly();
    let rejected: Result<(), String> = orderly.reject_request_with_customer_reason("Product is out of stock");
    assert!(rejected.is_err());
    assert_eq!(rejected.unwrap_err(), "Product is out of stock");
    assert_eq!(orderly.rejection_reasons, vec!["Product is out of stock".to_string()]);
}

// req:req:system-2 hash:b40be817
#[test]
fn req_system_2_b40be817() {
    let orderly = sample_orderly();
    let customer = sample_customer();
    let browsed = orderly.customer_browses_catalog(&customer);
    assert_eq!(browsed.len(), 1);
    assert_eq!(browsed[0].name, "Tea");
}

// req:req:system-3 hash:77cf84bb
#[test]
fn req_system_3_77cf84bb() {
    let order = sample_order();
    assert_eq!(order.customer_id, "cust-1");
    assert_eq!(order.state, OrderState::Placed);
}

// req:req:system-4 hash:4e85ad93
#[test]
fn req_system_4_4e85ad93() {
    let orderly = sample_orderly();
    let order = sample_order();
    let payment = orderly.customer_pays_through_payment(&order);
    assert_eq!(payment.order_id, "ord-1");
}

// req:req:system-5 hash:2cf777e3
#[test]
fn req_system_5_2cf777e3() {
    let mut shipment = Shipment { id: "ship-1".to_string(), order_id: "ord-1".to_string(), buyer_id: "cust-1".to_string(), state: ShipmentState::LeftWarehouse, tracking_links: Vec::new(), delivery_attempts: 0, buyer_refunded: false };
    assert!(shipment.received_by_customer("cust-1"));
    assert_eq!(shipment.state, ShipmentState::Delivered);
}

// req:req:system-6 hash:f1885d6c
#[test]
fn req_system_6_f1885d6c() {
    let orderly = sample_orderly();
    let order = sample_order();
    let return_request = orderly.unwanted_goods_go_back_through_returns(&order);
    assert_eq!(return_request.order_id, "ord-1");
}

// req:req:system-7 hash:52b74dfb
#[test]
fn req_system_7_52b74dfb() {
    let orderly = sample_orderly();
    assert_eq!(orderly.inventory.covers_warehouse_side(), "Inventory covers the warehouse side of Orderly");
}

// req:req:system-8 hash:9d522f0b
#[test]
fn req_system_8_9d522f0b() {
    let orderly = sample_orderly();
    assert!(orderly.operators_manage_with_admin_cli());
}

// req:req:system-9 hash:012251dd
#[test]
fn req_system_9_012251dd() {
    let orderly = sample_orderly();
    let mut order = sample_order();
    order.state = OrderState::Paid;
    let _shipment = order.create_shipment_when_paid_and_in_stock(&orderly.inventory, "ship-1").unwrap();
    assert!(order.traceable_from_placement_to_delivery_or_return());
}
