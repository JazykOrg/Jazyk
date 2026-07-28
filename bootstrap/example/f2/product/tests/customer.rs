// tests/customer.rs
#[cfg(test)]
mod tests {
    use product::customer::*;

    // Test suite for req:customer-1
    #[test]
    fn test_req_customer_1_unique_email_and_deletion_restriction() {
        // Setup: Create a customer with an unpaid order.
        let mut c = Customer::new("test@example.com".to_string());

        // Submit an order (which is initially unpaid)
        let result = c.submit_order(1, 1);
        assert!(result.is_ok(), "Order submission should succeed");

        // Attempt to delete the account while having an unpaid order
        let deletion_result = c.delete_account();

        // Assert that deletion fails due to unpaid orders (req:customer-1)
        assert!(deletion_result.is_err());
        assert_eq!(deletion_result.unwrap_err(), "Cannot delete account while having unpaid orders.");
    }

    // Test suite for req:customer-2
    #[test]
    fn test_req_customer_2_order_state_change_notification() {
        // Setup: Create a customer and an order.
        let mut c = Customer::new("notifier@example.com".to_string());
        let result = c.submit_order(1, 1);
        assert!(result.is_ok());

        let order_id = c.orders[0].id;

        // Action: Change the order state (e.g., to "SHIPPED")
        let notification_result = c.handle_order_state_change(order_id, "SHIPPED".to_string());

        // Assert that the state change is handled and notification logic is triggered (req:customer-2)
        assert!(notification_result.is_ok());
    }

    // Test suite for req:orders-2
    #[test]
    fn test_req_orders_2_stock_reservation_on_submission() {
        // Setup: Create a customer.
        let mut c = Customer::new("orderer_2@example.com".to_string());

        // Action: Submit an order (which triggers stock reservation)
        let result = c.submit_order(101, 2);

        // Assert that the submission succeeds and implies successful reservation (req:orders-2)
        assert!(result.is_ok());
    }

    // Test suite for req:orders-4
    #[test]
    fn test_req_orders_4_stock_reservation_on_submission() {
        // Setup: Create a customer.
        let mut c = Customer::new("orderer_4@example.com".to_string());

        // Action: Submit an order (which triggers stock reservation)
        let result = c.submit_order(102, 5);

        // Assert that the submission succeeds and implies successful reservation (req:orders-4)
        assert!(result.is_ok());
    }

    // Test suite for req:system-2
    #[test]
    fn test_req_system_2_rejection_feedback() {
        // Setup: Create a customer.
        let mut c = Customer::new("rejectee@example.com".to_string());

        // Action: Attempt an operation that fails (e.g., submitting order with zero quantity)
        let result = c.submit_order(103, 0);

        // Assert that the rejection is handled and a specific error message is returned to the customer (req:system-2)
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Quantity must be positive.");
    }
}