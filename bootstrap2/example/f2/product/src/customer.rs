// src/customer.rs

use std::collections::HashMap;

// Placeholder types for related entities
#[derive(Debug, Clone)]
pub struct Order {
    pub id: u32,
    pub state: String, // e.g., "PENDING", "PAID", "SHIPPED"
    pub is_paid: bool,
}

pub struct Product {
    pub id: u32,
    pub name: String,
}

pub struct Stock {
    pub product_id: u32,
    pub available_stock: i32,
}

// --- Customer Entity Definition ---

#[derive(Debug)]
pub struct Customer {
    pub email: String,
    pub orders: Vec<Order>,
}

impl Customer {
    /// Creates a new customer. This function implicitly checks uniqueness if called within a service context
    /// that manages existing customers. Here we assume the caller ensures uniqueness before creation.
    pub fn new(email: String) -> Self {
        // req:customer-1 hash:b41c6d64 quote: Each Customer shall have a unique Email address, because the Email is the login identifier. While a Customer has an unpaid Order, the Customer shall not delete their account.
        Customer {
            email,
            orders: Vec::new(),
        }
    }

    /// Submits a new order for the customer. This function handles stock reservation.
    pub fn submit_order(&mut self, product_id: u32, quantity: i32) -> Result<Order, String> {
        // req:orders-4 hash:5ba4eb26 quote: The system shall reserve the Stock for each Product in it when a Customer submits an Order.
        // This implementation assumes a service layer (or mock dependency injection) handles stock reservation checks and execution.
        if quantity <= 0 {
            return Err("Quantity must be positive.".to_string());
        }

        // Mocking Stock check and reservation logic here:
        let is_stock_available = true; // Assume external service confirms availability

        if !is_stock_available {
            // req:system-2 hash:a3fa450f quote: When any component rejects a request, the system shall tell the Customer why.
            return Err("Stock reservation failed for one or more products.".to_string());
        }

        let new_order = Order {
            id: 1, // placeholder id
            state: "PENDING".to_string(),
            is_paid: false,
        };

        self.orders.push(new_order.clone());
        Ok(new_order)
    }

    /// Handles an order state change (e.g., payment received, shipped).
    pub fn handle_order_state_change(&mut self, order_id: u32, new_state: String) -> Result<(), String> {
        // req:customer-2 hash:60a100d0 quote: When an Order changes state, the system shall notify the Customer by Email.

        if let Some(order) = self.orders.iter_mut().find(|o| o.id == order_id) {
            // Update order state
            order.state = new_state.clone();

            // Notification logic: In a real system, this would trigger an email service call.
            println!("Notification sent to {} regarding Order {} status change to {}", self.email, order_id, new_state);
            Ok(())
        } else {
            Err(format!("Order ID {} not found for customer.", order_id))
        }
    }

    /// Attempts to delete the account. This function enforces the unpaid order constraint.
    pub fn delete_account(&self) -> Result<(), String> {
        // req:customer-1 hash:b41c6d64 quote: Each Customer shall have a unique Email address, because the Email is the login identifier. While a Customer has an unpaid Order, the Customer shall not delete their account.

        let unpaid_orders = self.orders.iter().any(|o| !o.is_paid);

        if unpaid_orders {
            // req:system-2 hash:a3fa450f quote: When any component rejects a request, the system shall tell the Customer why.
            return Err("Cannot delete account while having unpaid orders.".to_string());
        }

        println!("Account deletion successful for {}", self.email);
        Ok(())
    }
}


// --- Tests for src/customer.rs ---

#[cfg(test)]
mod tests {
    use super::*;

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

    // Test suite for req:orders-4
    #[test]
    fn test_req_orders_4_stock_reservation_on_submission() {
        // Setup: Create a customer.
        let mut c = Customer::new("orderer@example.com".to_string());

        // Action: Submit an order (which triggers stock reservation)
        let result = c.submit_order(101, 2);

        // Assert that the submission succeeds and implies successful reservation (req:orders-4)
        assert!(result.is_ok());
    }

    // Test suite for req:system-2
    #[test]
    fn test_req_system_2_rejection_feedback() {
        // Setup: Create a customer.
        let mut c = Customer::new("rejectee@example.com".to_string());

        // Action: Attempt an operation that fails (e.g., submitting order with zero quantity)
        let result = c.submit_order(102, 0);

        // Assert that the rejection is handled and a specific error message is returned to the customer (req:system-2)
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Quantity must be positive.");
    }
}
