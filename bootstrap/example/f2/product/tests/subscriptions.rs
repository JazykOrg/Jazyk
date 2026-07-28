// tests/subscriptions.rs
#[cfg(test)]
mod tests {
    use product::subscriptions::*;

    // Helper function to simulate the environment setup for testing
    fn setup_subscription() -> Subscription {
        let sub_id = "sub-123".to_string();
        let customer_id = "cust-abc".to_string();
        let product_sku = "PROD-A".to_string();
        Subscription::new(sub_id, customer_id, product_sku)
    }

    // req:roadmap-3 hash:bd415022 The system shall support subscriptions, if enough Customers ask.
    #[test]
    fn req_roadmap_3_bd415022_subscription_creation_and_initial_state() {
        // Arrange
        let subscription = setup_subscription();

        // Act & Assert
        assert_eq!(subscription.status, SubscriptionStatus::PendingActivation, "New subscription should be pending activation.");
        assert!(!subscription.is_active(), "Newly created subscription should not be active before status update.");
    }

    // req:roadmap-3 hash:bd415022 The system shall support subscriptions, if enough Customers ask.
    #[test]
    fn req_roadmap_3_bd415022_subscription_status_transitions() {
        // Arrange
        let mut subscription = setup_subscription();

        // Act 1: Activate the subscription
        subscription.update_status(SubscriptionStatus::Active);

        // Assert 1
        assert!(subscription.is_active(), "Subscription should be active after status update.");
        assert_eq!(subscription.status, SubscriptionStatus::Active, "Status must be Active.");

        // Act 2: Deactivate/Cancel the subscription
        subscription.update_status(SubscriptionStatus::Canceled);

        // Assert 2
        assert!(!subscription.is_active(), "Subscription should not be active after cancellation.");
        assert_eq!(subscription.status, SubscriptionStatus::Canceled, "Status must be Canceled.");

        // Act 3: Expire the subscription (simulating time passing)
        subscription.update_status(SubscriptionStatus::Expired);

        // Assert 3
        assert!(!subscription.is_active(), "Subscription should not be active after expiration.");
        assert_eq!(subscription.status, SubscriptionStatus::Expired, "Status must be Expired.");
    }
}