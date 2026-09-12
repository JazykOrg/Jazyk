use product::order::{Order, OrderItem};
use product::product::Product;

#[test]
// req:req:orders-2 hash:bfac24ba
fn req_orders_2_bfac24ba_order_contains_catalog_products() {
    let catalog_product = Product::new(1, "Catalog product".to_string(), 12.50, 7);
    assert_eq!(catalog_product.catalog_category_id, 7, "test product must come from a catalog category");

    let order = Order::new(
        "O-catalog-products".to_string(),
        "C-catalog-products".to_string(),
        vec![OrderItem {
            product_id: catalog_product.id.to_string(),
            quantity: 1,
            placement_price: catalog_product.price,
        }],
    );
    assert!(!order.items.is_empty(), "an order must contain one or more products");
    assert_eq!(order.items[0].product_id, catalog_product.id.to_string());

    let empty_order = Order::new(
        "O-empty".to_string(),
        "C-catalog-products".to_string(),
        Vec::new(),
    );
    assert!(
        !empty_order.items.is_empty(),
        "Order::new must reject or prevent an order with zero products"
    );
}
