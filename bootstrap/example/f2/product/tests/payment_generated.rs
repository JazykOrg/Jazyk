use product::payment::{customer_pays_through_payment, order_management, Payment, PaymentStatus};

fn setup_order() -> order_management::Order {
    order_management::Order {
        id: "ord-test".to_string(),
        is_paid: false,
        status: order_management::OrderStatus::PendingPayment,
    }
}

fn setup_payment(attempts: u8) -> Payment {
    Payment {
        id: "pay-test".to_string(),
        amount: 10.0,
        status: PaymentStatus::Pending,
        order_id: "ord-test".to_string(),
        attempts,
    }
}

#[test]
// req:req:payment-1 hash:48c13b8e
fn req_payment_1_48c13b8e() {
    let payment = Payment::new("PAY-1", "ORDER-1", 25.00);
    assert!(payment.settles_order("ORDER-1"));
    assert!(!payment.settles_order("OTHER-ORDER"));
}

#[test]
// req:req:payment-2 hash:2f52f7b4
fn req_payment_2_2f52f7b4() {
    let mut order = setup_order();
    let payment = Payment {
        id: "p2".to_string(),
        amount: 50.0,
        status: PaymentStatus::Confirmed,
        order_id: "ord-test".to_string(),
        attempts: 1,
    };

    let result = order_management::confirm_payment(&mut order, &payment);

    assert!(result.is_ok(), "confirmed payment should be accepted");
    assert!(order.is_paid, "confirmed payment must mark the order as paid");
    assert_eq!(order.status, order_management::OrderStatus::Paid);
}

#[test]
// req:req:payment-4 hash:6f3f1877
fn req_payment_4_6f3f1877() {
    let mut order = setup_order();
    let mut payment = setup_payment(0);
    let mut customer_notifier = false;

    order_management::handle_failed_payment(&mut order, &mut payment, &mut customer_notifier).unwrap();
    assert_eq!(order.status, order_management::OrderStatus::PendingPayment);
    order_management::handle_failed_payment(&mut order, &mut payment, &mut customer_notifier).unwrap();
    assert_eq!(order.status, order_management::OrderStatus::PendingPayment);
    order_management::handle_failed_payment(&mut order, &mut payment, &mut customer_notifier).unwrap();

    assert_eq!(order.status, order_management::OrderStatus::OnHold);
}

#[test]
// req:req:payment-5 hash:2793ad62
fn req_payment_5_2793ad62() {
    let mut order = setup_order();
    let mut payment = setup_payment(2);
    let mut customer_notifier = false;

    order_management::handle_failed_payment(&mut order, &mut payment, &mut customer_notifier).unwrap();

    assert!(customer_notifier, "third failed Payment must notify the Customer");
}

#[test]
// req:req:returns-3 hash:ebd60fa9
fn req_returns_3_ebd60fa9() {
    let mut payment = Payment {
        id: "refund-payment".to_string(),
        amount: 10.0,
        status: PaymentStatus::Confirmed,
        order_id: "ord-test".to_string(),
        attempts: 1,
    };

    payment.refund().unwrap();

    assert_eq!(payment.status, PaymentStatus::Refunded);
}

#[test]
// req:req:system-4 hash:4e85ad93
fn req_system_4_4e85ad93() {
    let customer_payment = customer_pays_through_payment("C-1", "O-1", 42.00);

    assert_eq!(customer_payment.customer_id, "C-1");
    assert!(customer_payment.payment.settles_order("O-1"));
}
