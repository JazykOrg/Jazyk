use chrono::{Duration, Utc};
use product::order::{Order, OrderItem, OrderStatus};

fn setup_order() -> Order {
    Order::new(
        "O-payment-deadline".to_string(),
        "C-payment-deadline".to_string(),
        vec![OrderItem {
            product_id: "P1".to_string(),
            quantity: 1,
            placement_price: 10.0,
        }],
    )
}

#[test]
fn req_orders_4_29f2c263_cancels_only_unpaid_order_after_21_days() {
    let mut unpaid_order = setup_order();
    unpaid_order.created_at = Utc::now() - Duration::days(22);

    assert!(
        unpaid_order.check_payment_deadline(),
        "an unpaid order older than 21 days must be cancelled"
    );
    assert_eq!(unpaid_order.status, OrderStatus::Canceled);

    let mut paid_order = setup_order();
    paid_order.created_at = Utc::now() - Duration::days(22);
    paid_order.mark_as_paid();

    assert!(
        !paid_order.check_payment_deadline(),
        "a paid order must not be cancelled for missing the payment deadline"
    );
    assert_eq!(paid_order.status, OrderStatus::Paid);
}
