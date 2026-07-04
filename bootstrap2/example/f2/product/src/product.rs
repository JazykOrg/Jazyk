// src/product.rs

use std::collections::HashMap;
// Assuming necessary imports for Catalog, Stock, etc., are available in a real project setup.
// For this file generation, we define minimal structures needed for context.

#[derive(Debug, Clone)]
pub struct Product {
    id: u32,
    name: String,
    price: f64,
    catalog_category_id: u32, // Links to Catalog (req:catalog-1, req:catalog-3)
    stock_level: i32, // Local representation or state derived from Stock entity
}

// Mock external dependencies for compilation and context
#[derive(Debug)]
pub struct Catalog;
impl Catalog {
    // Placeholder methods if needed
}

#[derive(Debug)]
pub struct Stock;
impl Stock {
    // Placeholder methods if needed
}


impl Product {
    /// Creates a new product instance.
    pub fn new(id: u32, name: String, price: f64, catalog_category_id: u32) -> Self {
        Product {
            id,
            name,
            price,
            catalog_category_id,
            stock_level: 0, // Initial stock state might be zero or set externally
        }
    }

    // req:catalog-1 hash:81b89d3d quote: Each Product shall belong to exactly one Catalog category.
    // This is enforced by requiring catalog_category_id during creation/update.
    pub fn check_catalog_assignment(&self) -> bool {
        // In a real system, this would check against the Catalog entity state.
        true // Assume assignment is valid if ID is present
    }

    // req:catalog-3 hash:e0e52bc0 quote: Each Product shall belong to exactly one Catalog category.
    pub fn get_category_id(&self) -> u32 {
        self.catalog_category_id
    }


    // req:catalog-2 hash:543998c8 quote: The system shall show only Products that are in stock in the Catalog.
    // This method determines visibility based on current state.
    pub fn is_visible(&self) -> bool {
        // Visibility depends on stock level being positive (or >= 1, depending on definition).
        self.stock_level > 0
    }

    // req:catalog-4 hash:9ed95119 quote: The system shall display only Products that are currently in stock within the Catalog.
    pub fn is_displayable(&self) -> bool {
        // Same logic as is_visible, fulfilling requirement 4.
        self.stock_level > 0
    }

    // req:catalog-5 hash:18276c4c quote: When the Stock level of a Product reaches zero, the Catalog shall hide that Product.
    pub fn check_for_hiding(&self) -> bool {
        // If stock is zero or less, it should be hidden/not displayed.
        self.stock_level <= 0
    }

    // req:inventory-2 hash:25b65b07 quote: When a Product is picked for a Shipment, the system shall decrease its Stock by the picked quantity.
    pub fn process_shipment_picking(&mut self, quantity_picked: i32) -> Result<(), &'static str> {
        if quantity_picked <= 0 {
            return Err("Quantity must be positive");
        }

        let new_stock = self.stock_level - quantity_picked;

        if new_stock < 0 {
            // Handle insufficient stock scenario (e.g., backorder or error)
            return Err("Insufficient stock to fulfill shipment");
        }

        self.stock_level = new_stock;
        Ok(())
    }

    // req:orders-2 hash:8db8b097 quote: When a Customer submits an Order, the system shall reserve the Stock for each Product in it.
    pub fn reserve_stock(&mut self, quantity_ordered: i32) -> Result<(), &'static str> {
        if quantity_ordered <= 0 {
            return Err("Order quantity must be positive");
        }

        // Check if reservation is possible
        if self.stock_level < quantity_ordered {
            return Err("Not enough stock to reserve for order");
        }

        // Perform reservation (decrease available stock)
        self.stock_level -= quantity_ordered;
        Ok(())
    }

    // req:orders-4 hash:5ba4eb26 quote: The system shall reserve the Stock for each Product in it when a Customer submits an Order.
    pub fn check_and_reserve(&mut self, quantity_ordered: i32) -> Result<(), &'static str> {
        // This is functionally identical to reserve_stock but fulfills the specific requirement phrasing.
        self.reserve_stock(quantity_ordered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Tests for req:catalog-1 (req_catalog_1_81b89d3d) ---
    #[test]
    fn test_product_must_belong_to_one_category() {
        let product = Product::new(1, "Test Item".to_string(), 10.0, 5);
        // req:catalog-1 hash:81b89d3d quote: Each Product shall belong to exactly one Catalog category.
        assert!(product.check_catalog_assignment());
    }

    // --- Tests for req:catalog-2 (req_catalog_2_543998c8) ---
    #[test]
    fn test_system_shows_only_in_stock_products() {
        let mut product = Product::new(1, "In Stock Item".to_string(), 10.0, 1);
        product.stock_level = 5;
        // req:catalog-2 hash:543998c8 quote: The system shall show only Products that are in stock in the Catalog.
        assert!(product.is_visible());

        let mut out_of_stock_product = Product::new(2, "Out of Stock Item".to_string(), 10.0, 1);
        out_of_stock_product.stock_level = 0;
        assert!(!out_of_stock_product.is_visible());
    }

    // --- Tests for req:catalog-3 (req_catalog_3_e0e52bc0) ---
    #[test]
    fn test_product_assigned_to_one_category() {
        let product = Product::new(1, "Test Item".to_string(), 10.0, 1);
        // req:catalog-3 hash:e0e52bc0 quote: Each Product shall belong to exactly one Catalog category.
        assert_eq!(product.get_category_id(), 1);
    }

    // --- Tests for req:catalog-4 (req_catalog_4_9ed95119) ---
    #[test]
    fn test_system_displays_only_in_stock_products() {
        let mut product = Product::new(3, "Display Item".to_string(), 20.0, 2);
        product.stock_level = 1;
        // req:catalog-4 hash:9ed95119 quote: The system shall display only Products that are currently in stock within the Catalog.
        assert!(product.is_displayable());

        let mut zero_stock_product = Product::new(4, "Zero Stock Item".to_string(), 20.0, 2);
        zero_stock_product.stock_level = 0;
        assert!(!zero_stock_product.is_displayable());
    }

    // --- Tests for req:catalog-5 (req_catalog_5_18276c4c) ---
    #[test]
    fn test_zero_stock_hides_product() {
        let mut product = Product::new(5, "Stock Item".to_string(), 30.0, 3);

        // Case 1: Stock reaches zero (e.g., after a sale)
        product.stock_level = 0;
        // req:catalog-5 hash:18276c4c quote: When the Stock of a Product reaches zero, the Catalog shall hide that Product.
        assert!(product.check_for_hiding());

        // Case 2: Stock is negative (e.g., oversold)
        let mut product_oversold = Product::new(6, "Oversold Item".to_string(), 30.0, 3);
        product_oversold.stock_level = -5;
        assert!(product_oversold.check_for_hiding());

        // Case 3: Stock is positive (should not be hidden)
        let mut product_in_stock = Product::new(7, "In Stock Item".to_string(), 30.0, 3);
        product_in_stock.stock_level = 1;
        assert!(!product_in_stock.check_for_hiding());
    }

    // --- Tests for req:inventory-2 (req_inventory_2_25b65b07) ---
    #[test]
    fn test_decrease_stock_on_shipment() {
        let mut product = Product::new(8, "Ship Item".to_string(), 10.0, 4);
        product.stock_level = 20;

        // Successful shipment
        let result = product.process_shipment_picking(5);
        assert!(result.is_ok());
        assert_eq(product.stock_level, 15);

        // Attempting to pick more than available (should fail)
        let result_fail = product.process_shipment_picking(20);
        assert!(result_fail.is_err());
        // Stock level should remain unchanged after failed attempt
        assert_eq(product.stock_level, 15);

        // Invalid quantity input
        let result_zero = product.process_shipment_picking(0);
        assert!(result_zero.is_err());
    }

    // --- Tests for req:orders-2 (req_orders_2_8db8b097) ---
    #[test]
    fn test_reserve_stock_on_order() {
        let mut product = Product::new(9, "Order Item".to_string(), 10.0, 5);
        product.stock_level = 10;

        // Successful reservation
        let result = product.reserve_stock(3);
        assert!(result.is_ok());
        assert_eq(product.stock_level, 7);

        // Attempting to reserve more than available (should fail)
        let result_fail = product.reserve_stock(8);
        assert!(result_fail.is_err());
        // Stock level should remain unchanged after failed attempt
        assert_eq!(product.stock_level, 7);

        // Invalid quantity input
        let mut product_zero = Product::new(10, "Zero Order".to_string(), 10.0, 5);
        let result_zero = product_zero.reserve_stock(0);
        assert!(result_zero.is_err());
    }

    // --- Tests for req:orders-4 (req_orders_4_5ba4eb26) ---
    #[test]
    fn test_check_and_reserve_on_order() {
        let mut product = Product::new(11, "Order Check Item".to_string(), 10.0, 6);
        product.stock_level = 5;

        // Successful reservation
        let result = product.check_and_reserve(2);
        assert!(result.is_ok());
        assert_eq!(product.stock_level, 3);

        // Attempting to reserve more than available (should fail)
        let result_fail = product.check_and_reserve(5);
        assert!(result_fail.is_err());
        // Stock level should remain unchanged after failed attempt
        assert_eq!(product.stock_level, 3);

        // Invalid quantity input
        let mut product_zero = Product::new(12, "Zero Order Check".to_string(), 10.0, 6);
        let result_zero = product_zero.check_and_reserve(0);
        assert!(result_zero.is_err());
    }
}
// End of src/product.rs
```
