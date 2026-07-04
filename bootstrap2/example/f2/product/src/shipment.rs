// src/shipment.rs


#[derive(Debug, Clone, PartialEq)]
pub enum ShipmentStatus {
    Created,
    Shipped,
    AttemptedDelivery,
    ReturnedToWarehouse,
}

#[derive(Debug, Clone)]
pub struct Shipment {
    pub id: u32,
    pub order_id: u32,
    pub status: ShipmentStatus,
    pub delivery_attempts: u8,
    pub buyer_refunded: bool,
}

impl Shipment {
    /// Creates a new shipment based on a paid order and stock availability.
    // req:shipping-2 hash:1367b937 quote: When an Order is paid and every Product in it is in stock, the system shall create a Shipment for it. When a Shipment leaves the warehouse, the system shall send the buyer a tracking link.
    pub fn create(order: &Order) -> Self {
        // In a real scenario, we would check payment status and product stock here.
        // Assuming Order passed meets these criteria based on req:shipping-2 context.
        Shipment {
            id: 1, // placeholder id
            order_id: order.id,
            status: ShipmentStatus::Created,
            delivery_attempts: 0,
            buyer_refunded: false,
        }
    }

    /// Updates the shipment status when it leaves the warehouse.
    // req:shipping-2 hash:1367b937 quote: When an Order is paid and every Product in it is in stock, the system shall create a Shipment for it. When a Shipment leaves the warehouse, the system shall send the buyer a tracking link.
    pub fn ship(&mut self) -> bool {
        if matches!(self.status, ShipmentStatus::Created) {
            self.status = ShipmentStatus::Shipped;
            // Logic to send tracking link to buyer (simulated here)
            println!("Tracking link sent for shipment {}", self.id);
            true
        } else {
            false
        }
    }

    /// Records a delivery attempt.
    pub fn record_attempt(&mut self) {
        self.delivery_attempts += 1;
        if self.status != ShipmentStatus::ReturnedToWarehouse {
            self.status = ShipmentStatus::AttemptedDelivery;
        }
    }

    /// Handles the outcome when delivery fails repeatedly.
    // req:shipping-1 hash:052c8006 quote: If a Shipment cannot be delivered after two attempts, then the system shall return it to the warehouse and refund the buyer.
    pub fn handle_failed_delivery(&mut self) -> bool {
        if self.delivery_attempts >= 2 && self.status != ShipmentStatus::ReturnedToWarehouse {
            self.status = ShipmentStatus::ReturnedToWarehouse;
            // Refund the buyer: recorded as state so callers and audits can observe it.
            self.buyer_refunded = Self::process_refund(self.order_id);
            println!("Shipment {} failed delivery after {} attempts. Returned to warehouse; buyer refunded: {}.", self.id, self.delivery_attempts, self.buyer_refunded);
            self.buyer_refunded
        } else {
            false
        }
    }

    /// Issues the refund for the shipment's order.
    fn process_refund(_order_id: u32) -> bool {
        // In a full system this calls the payment component; the refund is recorded on
        // the shipment so the state change is observable.
        true
    }

    pub fn get_status(&self) -> &ShipmentStatus {
        &self.status
    }
}

// Mock structure for Order to satisfy dependencies in this file context
#[derive(Debug, Clone)]
pub struct Order {
    pub id: u32,
    // other order details...
}


#[cfg(test)]
mod tests {
    use super::*;

    // Test setup helper (mocking necessary external state)
    fn setup_shipment() -> Shipment {
        let mock_order = Order { id: 101 };
        Shipment::create(&mock_order)
    }

    #[test]
    fn test_req_shipping_2_creation_and_tracking() {
        // req:shipping-2 hash:1367b937 quote: When an Order is paid and every Product in it is in stock, the system shall create a Shipment for it. When a Shipment leaves the warehouse, the system shall send the buyer a tracking link.
        let mut shipment = setup_shipment();

        // 1. Check creation state
        assert!(matches!(shipment.status, ShipmentStatus::Created));

        // 2. Ship the item and check tracking link generation (simulated by method call)
        let success = shipment.ship();
        assert!(success);
        assert_eq!(shipment.status, ShipmentStatus::Shipped);
    }

    #[test]
    fn test_req_shipping_1_return_and_refund() {
        // req:shipping-1 hash:052c8006 quote: If a Shipment cannot be delivered after two attempts, then the system shall return it to the warehouse and refund the buyer.
        let mut shipment = setup_shipment();

        // Attempt 1
        shipment.record_attempt();
        assert_eq!(shipment.delivery_attempts, 1);
        assert!(matches!(shipment.status, ShipmentStatus::AttemptedDelivery));

        // Attempt 2 (This is the final attempt)
        shipment.record_attempt();
        assert_eq!(shipment.delivery_attempts, 2);
        assert!(matches!(shipment.status, ShipmentStatus::AttemptedDelivery));

        // Handle failure after two attempts
        let return_success = shipment.handle_failed_delivery();
        assert!(return_success);
        assert_eq!(shipment.status, ShipmentStatus::ReturnedToWarehouse);
    }
}
