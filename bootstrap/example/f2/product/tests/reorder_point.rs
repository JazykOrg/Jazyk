#[path = "../src/reorder_point.rs"]
mod reorder_point;

use reorder_point::{ProductId, ProductStock, ReorderPoint, RestockTask};

#[test]
fn req_glossary_3_0d257092() {
    let source = include_str!("../src/reorder_point.rs");
    let reorder_point = ReorderPoint { stock_level: 5 };
    let stock_at_threshold = ProductStock { product_id: ProductId::from("SKU-1042"), stock: 5 };
    let stock_above_threshold = ProductStock { product_id: ProductId::from("SKU-1042"), stock: 6 };

    assert!(source.contains("Reorder point: the Stock level that triggers restocking."));
    assert_eq!(ReorderPoint::DEFINITION, "Reorder point: the Stock level that triggers restocking.");
    assert_eq!(reorder_point.stock_level, 5);
    assert!(reorder_point.is_stock_level_that_triggers_restocking(&stock_at_threshold));
    assert!(!reorder_point.is_stock_level_that_triggers_restocking(&stock_above_threshold));
}

#[test]
fn req_inventory_3_900f93cb() {
    let reorder_point = ReorderPoint { stock_level: 5 };
    let stock = ProductStock { product_id: ProductId::from("SKU-1042"), stock: 5 };
    let above_reorder_point = ProductStock { product_id: ProductId::from("SKU-1042"), stock: 6 };

    let task = reorder_point.create_restock_task_when_reached(&stock);

    assert_eq!(
        task,
        Some(RestockTask {
            product_id: ProductId::from("SKU-1042"),
            current_stock: 5,
            reorder_point: 5,
        })
    );
    assert_eq!(reorder_point.create_restock_task_when_reached(&above_reorder_point), None);
}
