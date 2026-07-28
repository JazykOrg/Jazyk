// tests/product.rs
use product::product::*;

#[test]
fn test_product_must_belong_to_one_category() {
    let product = Product::new(1, "Test Item".to_string(), 10.0, 5);
    // req:catalog-1 hash:81b89d3d quote: Each Product shall belong to exactly one Catalog category.
    assert!(product.check_catalog_assignment());
}

#[test]
fn test_system_shows_only_in_stock_products() {
    // Test case 1: In stock product should be visible
    let mut product_in_stock = Product::new(1, "In Stock Item".to_string(), 10.0, 1);
    product_in_stock.stock_level = 5;
    // req:catalog-2 hash:543998c8 quote: The system shall show only Products that are in stock in the Catalog.
    assert!(product_in_stock.is_visible());

    // Test case 2: Out of stock product should not be visible
    let mut out_of_stock_product = Product::new(2, "Out of Stock Item".to_string(), 10.0, 1);
    out_of_stock_product.stock_level = 0;
    assert!(!out_of_stock_product.is_visible());
}

#[test]
fn test_product_assigned_to_one_category() {
    let product = Product::new(1, "Test Item".to_string(), 10.0, 1);
    // req:catalog-3 hash:e0e52bc0 quote: Each Product shall belong to exactly one Catalog category.
    assert_eq!(product.get_category_id(), 1);
}

#[test]
fn test_system_displays_only_in_stock_products() {
    // Test case 1: In stock product should be displayable
    let mut product = Product::new(3, "Display Item".to_string(), 20.0, 2);
    product.stock_level = 1;
    // req:catalog-4 hash:9ed95119 quote: The system shall display only Products that are currently in stock within the Catalog.
    assert!(product.is_displayable());

    // Test case 2: Zero stock product should not be displayable
    let mut zero_stock_product = Product::new(4, "Zero Stock Item".to_string(), 20.0, 2);
    zero_stock_product.stock_level = 0;
    assert!(!zero_stock_product.is_displayable());
}

#[test]
fn test_zero_stock_hides_product() {
    let mut product = Product::new(5, "Stock Item".to_string(), 30.0, 3);

    // Case 1: Stock reaches zero -> should be hidden
    product.stock_level = 0;
    // req:catalog-5 hash:18276c4c quote: When the Stock of a Product reaches zero, the Catalog shall hide that Product.
    assert!(product.check_for_hiding());

    // Case 2: Stock is negative -> should be hidden
    let mut product_oversold = Product::new(6, "Oversold Item".to_string(), 30.0, 3);
    product_oversold.stock_level = -5;
    assert!(product_oversold.check_for_hiding());

    // Case 3: Stock is positive -> should not be hidden
    let mut product_in_stock = Product::new(7, "In Stock Item".to_string(), 30.0, 3);
    product_in_stock.stock_level = 1;
    assert!(!product_in_stock.check_for_hiding());
}

#[test]
fn test_decrease_stock_on_shipment() {
    let mut product = Product::new(8, "Ship Item".to_string(), 10.0, 4);
    product.stock_level = 20;

    // Successful shipment
    let result = product.process_shipment_picking(5);
    assert!(result.is_ok());
    assert_eq!(product.stock_level, 15);

    // Attempting to pick more than available (should fail)
    let result_fail = product.process_shipment_picking(20);
    assert!(result_fail.is_err());
    // Stock level should remain unchanged after failed attempt
    assert_eq!(product.stock_level, 15);

    // Invalid quantity input (zero)
    let result_zero = product.process_shipment_picking(0);
    assert!(result_zero.is_err());
}

#[test]
fn test_reserve_stock_on_order() {
    let mut product = Product::new(9, "Order Item".to_string(), 10.0, 5);
    product.stock_level = 10;

    // Successful reservation
    let result = product.reserve_stock(3);
    assert!(result.is_ok());
    assert_eq!(product.stock_level, 7);

    // Attempting to reserve more than available (should fail)
    let result_fail = product.reserve_stock(8);
    assert!(result_fail.is_err());
    // Stock level should remain unchanged after failed attempt
    assert_eq!(product.stock_level, 7);

    // Invalid quantity input (zero)
    let mut product_zero = Product::new(10, "Zero Order".to_string(), 10.0, 5);
    let result_zero = product_zero.reserve_stock(0);
    assert!(result_zero.is_err());
}

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

    // Invalid quantity input (zero)
    let mut product_zero = Product::new(12, "Zero Order Check".to_string(), 10.0, 6);
    let result_zero = product_zero.check_and_reserve(0);
    assert!(result_zero.is_err());
}