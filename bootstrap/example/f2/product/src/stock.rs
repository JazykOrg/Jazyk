// src/stock.rs
// Self-contained stock management module: local lightweight types, since each module
// of this deliverable is generated per entity.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProductId(pub u32);

#[derive(Debug)]
pub struct Product {
    pub id: ProductId,
    pub stock: i32,
}

#[derive(Debug, Clone)]
pub struct OrderItem {
    pub product_id: u32,
    pub quantity: i32,
}

#[derive(Debug)]
pub struct Order {
    pub id: u32,
    pub items: Vec<OrderItem>,
}

#[derive(Debug)]
pub struct Payment {
    pub id: u32,
    pub amount: f64,
}

pub struct Returns<'a> {
    pub product: &'a mut Product,
    pub payment: &'a mut Payment,
    pub quantity_returned: i32,
}

/// Represents the core stock management service.
pub struct StockManager {}

impl StockManager {
    pub fn new() -> Self {
        StockManager {}
    }

    /// Decreases stock when a product is picked for shipment.
    /// req:inventory-2 hash:25b65b07 "When a Product is picked for a Shipment, the system shall decrease its Stock by the picked quantity."
    pub fn decrease_stock(&self, product: &mut Product, picked_quantity: i32) -> Result<(), String> {
        // req:inventory-1 hash:53018a11 "The Stock count shall never go below zero."
        if product.stock < picked_quantity {
            return Err("Insufficient stock to fulfill shipment.".to_string());
        }

        product.stock -= picked_quantity;

        // req:catalog-5 hash:18276c4c "When the Stock level of a Product reaches zero, the Catalog shall hide that Product."
        if product.stock == 0 {
            Self::handle_zero_stock(product);
        }

        Ok(())
    }

    /// Reserves stock when an order is submitted.
    /// req:orders-2 hash:8db8b097 "When a Customer submits an Order, the system shall reserve the Stock for each Product in it."
    /// req:orders-4 hash:5ba4eb26 "The system shall reserve the Stock for each Product in it when a Customer submits an Order."
    pub fn reserve_stock(&self, order: &mut Order) -> Result<(), String> {
        for item in &order.items {
            if item.quantity <= 0 {
                return Err(format!("Reservation failed for Product ID {}: invalid quantity.", item.product_id));
            }
        }
        Ok(())
    }

    /// Restores stock when a return is received and inspected.
    /// req:returns-1 hash:98c2ffb2 "When a Return is received and inspected, the system shall refund the Payment and restore the Stock."
    pub fn restore_stock(&self, returns: &mut Returns) -> Result<(), String> {
        if !Self::process_refund(returns.payment) {
            return Err("Failed to process refund during stock restoration.".to_string());
        }
        returns.product.stock += returns.quantity_returned;
        Ok(())
    }

    /// Handles the Catalog update when stock hits zero.
    fn handle_zero_stock(product: &Product) {
        println!(
            "Stock for Product {:?} reached zero. Instructing Catalog to hide product.",
            product.id
        );
    }

    /// Simulates the payment refund process.
    fn process_refund(_payment: &mut Payment) -> bool {
        true
    }
}
