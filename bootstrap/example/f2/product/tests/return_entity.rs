#[path = "../src/return_entity.rs"]
mod return_entity;

use chrono::{Duration, Utc};
use return_entity::{InventoryLedger, OrderDelivery, PaymentLedger, Return, ReturnLine};

fn line(product_id: &str, quantity: u32) -> ReturnLine {
    ReturnLine { product_id: product_id.to_string(), quantity }
}

fn delivery() -> OrderDelivery {
    OrderDelivery {
        order_id: "O-1".to_string(),
        customer_id: "C-1".to_string(),
        delivered_at: Utc::now(),
    }
}

#[test]
// req:req:returns-1 hash:5429692a
fn req_returns_1_5429692a() {
    let returned = Return::brings_goods_from_customer_back_into_inventory(
        "R-1",
        "C-1",
        "O-1",
        Utc::now(),
        vec![line("SKU-1042", 2)],
    );

    assert_eq!(returned.customer_id, "C-1");
    assert_eq!(returned.order_id, "O-1");
    assert_eq!(returned.lines, vec![line("SKU-1042", 2)]);
}

#[test]
// req:req:returns-2 hash:51943b2e
fn req_returns_2_51943b2e() {
    let delivery = delivery();

    let in_window = Return::open_for_order_within_30_days(
        "R-1",
        &delivery,
        delivery.delivered_at + Duration::days(30),
        vec![line("SKU-1042", 1)],
    );
    let late = Return::open_for_order_within_30_days(
        "R-late",
        &delivery,
        delivery.delivered_at + Duration::days(31),
        vec![line("SKU-1042", 1)],
    );

    assert!(in_window.is_some());
    assert!(late.is_none());
}

#[test]
// req:req:returns-3 hash:ebd60fa9
fn req_returns_3_ebd60fa9() {
    let mut returned = Return::brings_goods_from_customer_back_into_inventory(
        "R-1",
        "C-1",
        "O-1",
        Utc::now(),
        vec![line("SKU-1042", 2)],
    );
    let mut payments = PaymentLedger::default();

    assert!(returned.refund_payment_after_received_and_inspected(&mut payments).is_err());
    returned.mark_received_and_inspected();
    returned.refund_payment_after_received_and_inspected(&mut payments).unwrap();

    assert_eq!(payments.refunded_orders, vec!["O-1".to_string()]);
}

#[test]
// req:req:returns-4 hash:264fa475
fn req_returns_4_264fa475() {
    let mut returned = Return::brings_goods_from_customer_back_into_inventory(
        "R-1",
        "C-1",
        "O-1",
        Utc::now(),
        vec![line("SKU-1042", 2), line("SKU-2000", 1)],
    );
    let mut inventory = InventoryLedger::default();

    assert!(returned.restore_stock_after_received_and_inspected(&mut inventory).is_err());
    returned.mark_received_and_inspected();
    returned.restore_stock_after_received_and_inspected(&mut inventory).unwrap();

    assert_eq!(inventory.restored, vec![line("SKU-1042", 2), line("SKU-2000", 1)]);
}
