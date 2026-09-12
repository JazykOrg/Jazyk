#[path = "../src/restock_task.rs"]
mod restock_task;

use restock_task::{Inventory, ProductStock, RestockTask};

#[test]
fn req_inventory_3_900f93cb() {
    let mut inventory = Inventory::new();
    let product = ProductStock {
        product_id: "SKU-1042".to_string(),
        stock: 5,
        reorder_point: 5,
    };

    let task = inventory.evaluate_reorder_point(&product);

    assert_eq!(
        task,
        Some(RestockTask {
            product_id: "SKU-1042".to_string(),
            current_stock: 5,
            reorder_point: 5,
        })
    );
    assert_eq!(inventory.restock_tasks().len(), 1);
    assert_eq!(inventory.restock_tasks()[0].product_id, "SKU-1042");

    let above_reorder_point = ProductStock {
        product_id: "SKU-2000".to_string(),
        stock: 6,
        reorder_point: 5,
    };
    assert_eq!(inventory.evaluate_reorder_point(&above_reorder_point), None);
    assert_eq!(inventory.restock_tasks().len(), 1);
}
