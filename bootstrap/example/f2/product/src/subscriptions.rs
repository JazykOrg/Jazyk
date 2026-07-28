// src/subscriptions.rs

/// Represents the possible states of a subscription.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SubscriptionStatus {
    PendingActivation,
    Active,
    Canceled,
    Expired,
}

/// Represents a single customer subscription.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub id: String,
    pub customer_id: String,
    pub product_sku: String,
    pub start_date: chrono::NaiveDate,
    pub status: SubscriptionStatus,
}

impl Subscription {
    /// Creates a new subscription instance.
    pub fn new(id: String, customer_id: String, product_sku: String) -> Self {
        Subscription {
            id,
            customer_id,
            product_sku,
            start_date: chrono::Utc::now().date_naive(),
            status: SubscriptionStatus::PendingActivation,
        }
    }

    /// Updates the status of the subscription.
    pub fn update_status(&mut self, new_status: SubscriptionStatus) {
        self.status = new_status;
    }

    // req:roadmap-3 hash:bd415022 The system shall support subscriptions, if enough Customers ask.
    /// Checks if the subscription is currently active.
    pub fn is_active(&self) -> bool {
        self.status == SubscriptionStatus::Active
    }
}

// --- Tests for src/subscriptions.rs ---

#[cfg(test)]
mod tests {
    use super::*;

    /// Test case for req:roadmap-3 [req_roadmap_3_bd415022] The system shall support subscriptions, if enough Customers ask.
    #[test]
    fn test_subscription_creation_and_status() {
        // Arrange
        let sub_id = "sub-123".to_string();
        let customer_id = "cust-abc".to_string();
        let product_sku = "PROD-A".to_string();

        // Act
        let subscription = Subscription::new(sub_id, customer_id, product_sku);

        // Assert
        assert_eq!(subscription.status, SubscriptionStatus::PendingActivation, "New subscription should be pending activation.");
        assert!(!subscription.is_active(), "Newly created subscription should not be active.");
    }
}
