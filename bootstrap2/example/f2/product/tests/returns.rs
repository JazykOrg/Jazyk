// tests/returns.rs

use product::returns::{Return, payment::PaymentManager, stock::StockManager};

#[test]
// req:returns-1 hash:98c2ffb2 "When a Return is received and inspected, the system shall refund the Payment and restore the Stock."
fn test_process_return_refunds_and_restores_stock() {
    // Setup
    let mut payment_manager = PaymentManager {};
    let mut stock_manager = StockManager {};
    let item_id = 101;
    let quantity = 2;

    // Create and inspect the return
    let mut return_item = Return::new(item_id, quantity);
    return_item.is_inspected = true; // Simulate inspection completion

    // Action
    let result = return_item.process_return(&mut payment_manager, &mut stock_manager);

    // Assert
    assert!(result.is_ok(), "Processing the return should succeed.");
}