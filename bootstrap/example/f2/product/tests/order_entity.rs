#[path = "../src/order_entity.rs"]
mod order_entity;

use chrono::{Duration, Utc};
use order_entity::{AdminCli, Order, OrderStatus, ProductSnapshot};

fn item(id: &str, quantity: u32, price: f64) -> ProductSnapshot {
    ProductSnapshot {
        product_id: id.to_string(),
        quantity,
        placement_price: price,
        from_catalog: true,
        stock_reserved: false,
        in_stock: true,
    }
}

fn placed_order() -> Order {
    Order::place("O-1", "C-1", vec![item("P-1", 2, 12.50)], Utc::now())
}

#[test]
// req:req:admin-6 hash:2c3c595c
fn req_admin_6_2c3c595c() {
    let start = Utc::now() - Duration::days(3);
    let inside_one = Order::place("O-in-1", "C-1", vec![item("P-1", 1, 10.0)], start + Duration::days(1));
    let inside_two = Order::place("O-in-2", "C-2", vec![item("P-2", 1, 20.0)], start + Duration::days(2));
    let outside = Order::place("O-out", "C-3", vec![item("P-3", 1, 30.0)], start - Duration::days(1));
    let cli = AdminCli::with_orders(vec![inside_one.clone(), outside, inside_two.clone()]);

    let report = cli.report(start, start + Duration::days(3));

    assert_eq!(report.len(), 2);
    assert!(report.iter().any(|order| order.order_id == inside_one.order_id));
    assert!(report.iter().any(|order| order.order_id == inside_two.order_id));
}

#[test]
// req:req:customer-4 hash:7f25c49a
fn req_customer_4_7f25c49a() {
    let mut order = placed_order();
    assert!(!order.customer_can_delete_account());
    order.confirm_payment("PAY-1", Utc::now());
    assert!(order.customer_can_delete_account());
}

#[test]
// req:req:customer-5 hash:91839be5
fn req_customer_5_91839be5() {
    let mut order = placed_order();
    order.confirm_payment("PAY-1", Utc::now());
    assert!(order.email_notifications().iter().any(|message| message.contains("email:C-1")));
}

#[test]
// req:req:orders-1 hash:b4add94c
fn req_orders_1_b4add94c() {
    let order = placed_order();
    assert_eq!(order.customer_id, "C-1");
    assert_eq!(order.status, OrderStatus::Submitted);
    assert!(!order.items.is_empty());
}

#[test]
// req:req:orders-2 hash:bfac24ba
fn req_orders_2_bfac24ba() {
    let order = placed_order();
    assert!(!order.items.is_empty());
    assert!(order.items.iter().all(|item| item.from_catalog));
}

#[test]
// req:req:orders-3 hash:ebd76a10
fn req_orders_3_ebd76a10() {
    let mut order = placed_order();
    order.submit_and_reserve_stock();
    assert!(order.items.iter().all(|item| item.stock_reserved));
}

#[test]
// req:req:orders-4 hash:29f2c263
fn req_orders_4_29f2c263() {
    let placed = Utc::now() - Duration::days(22);
    let mut unpaid = Order::place("O-unpaid", "C-1", vec![item("P-1", 1, 10.0)], placed);
    assert!(unpaid.cancel_if_unpaid_after_21_days(Utc::now()));
    assert_eq!(unpaid.status, OrderStatus::Canceled);

    let mut paid = Order::place("O-paid", "C-1", vec![item("P-1", 1, 10.0)], placed);
    paid.confirm_payment("PAY-1", placed + Duration::days(1));
    assert!(!paid.cancel_if_unpaid_after_21_days(Utc::now()));
    assert_eq!(paid.status, OrderStatus::Paid);
}

#[test]
// req:req:orders-5 hash:9a77aa9a
fn req_orders_5_9a77aa9a() {
    let order = placed_order();
    assert_eq!(order.listed_products(), vec![("P-1".to_string(), 2, 12.50)]);
}

#[test]
// req:req:payment-1 hash:48c13b8e
fn req_payment_1_48c13b8e() {
    let mut order = placed_order();
    order.confirm_payment("PAY-1", Utc::now());
    assert_eq!(order.status, OrderStatus::Paid);
}

#[test]
// req:req:payment-2 hash:2f52f7b4
fn req_payment_2_2f52f7b4() {
    let mut order = placed_order();
    order.confirm_payment("PAY-1", Utc::now());
    assert!(order.trace_from_placement_to_delivery_or_return().iter().any(|entry| entry.note.contains("settled order")));
}

#[test]
// req:req:payment-3 hash:2e4285d5
fn req_payment_3_2e4285d5() {
    let order = placed_order();
    assert!(order.is_within_30_day_payment_window(order.placed_at + Duration::days(30)));
    assert!(!order.is_within_30_day_payment_window(order.placed_at + Duration::days(31)));
}

#[test]
// req:req:payment-4 hash:6f3f1877
fn req_payment_4_6f3f1877() {
    let mut order = placed_order();
    order.record_failed_payment(Utc::now());
    order.record_failed_payment(Utc::now());
    order.record_failed_payment(Utc::now());
    assert_eq!(order.status, OrderStatus::OnHold);
}

#[test]
// req:req:returns-2 hash:51943b2e
fn req_returns_2_51943b2e() {
    let mut order = placed_order();
    let delivered_at = Utc::now();
    order.mark_delivered(delivered_at);
    assert!(order.open_return_within_30_days("R-1", delivered_at + Duration::days(30)).is_some());

    let mut late_order = placed_order();
    late_order.mark_delivered(delivered_at);
    assert!(late_order.open_return_within_30_days("R-late", delivered_at + Duration::days(31)).is_none());
}

#[test]
// req:req:shipping-1 hash:f1081307
fn req_shipping_1_f1081307() {
    let mut order = placed_order();
    order.confirm_payment("PAY-1", Utc::now());
    let shipment = order.create_shipment_if_paid_and_in_stock("S-1", Utc::now()).expect("shipment is created");
    assert!(order.shipment_delivers_packed_order_to_buyer(&shipment));
}

#[test]
// req:req:shipping-2 hash:58de09dc
fn req_shipping_2_58de09dc() {
    let mut order = placed_order();
    order.confirm_payment("PAY-1", Utc::now());
    let shipment = order.create_shipment_if_paid_and_in_stock("S-1", Utc::now());
    assert!(shipment.is_some());
    assert_eq!(order.status, OrderStatus::Shipped);
}

#[test]
// req:req:system-3 hash:77cf84bb
fn req_system_3_77cf84bb() {
    let order = Order::place("O-system", "C-system", vec![item("P-1", 1, 9.99)], Utc::now());
    assert_eq!(order.customer_id, "C-system");
    assert_eq!(order.status, OrderStatus::Submitted);
}

#[test]
// req:req:system-9 hash:012251dd
fn req_system_9_012251dd() {
    let mut delivered = placed_order();
    delivered.confirm_payment("PAY-1", Utc::now());
    delivered.mark_delivered(Utc::now());
    let trace = delivered.trace_from_placement_to_delivery_or_return();
    assert_eq!(trace.first().unwrap().status, OrderStatus::Submitted);
    assert_eq!(trace.last().unwrap().status, OrderStatus::Delivered);

    let mut returned = placed_order();
    returned.mark_delivered(Utc::now());
    returned.open_return_within_30_days("R-1", Utc::now());
    assert_eq!(returned.trace_from_placement_to_delivery_or_return().last().unwrap().status, OrderStatus::Returned);
}
