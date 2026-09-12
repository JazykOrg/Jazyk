use chrono::{Duration, Utc};
use product::order::{Order, OrderItem, OrderStatus};

fn setup_order() -> Order {
    Order::new(
        "O-payment-3".to_string(),
        "C-payment-3".to_string(),
        vec![OrderItem {
            product_id: "P-payment-3".to_string(),
            quantity: 1,
            placement_price: 10.0,
        }],
    )
}

// req:payment-3 [req_payment_3_2e4285d5]: An Order shall be paid within 30 days of placement.
#[test]
fn req_payment_3_2e4285d5() {
    let mut unpaid_recent = setup_order();
    unpaid_recent.created_at = Utc::now() - Duration::days(29);
    assert!(
        unpaid_recent.check_long_term_deadline(),
        "an unpaid order still inside 30 days remains compliant"
    );

    let mut unpaid_late = setup_order();
    unpaid_late.created_at = Utc::now() - Duration::days(31);
    assert!(
        !unpaid_late.check_long_term_deadline(),
        "an unpaid order older than 30 days violates the payment deadline"
    );

    let mut paid_late = setup_order();
    paid_late.created_at = Utc::now() - Duration::days(31);
    paid_late.status = OrderStatus::Paid;
    assert!(
        paid_late.check_long_term_deadline(),
        "an order older than 30 days complies when it has been paid"
    );
}
