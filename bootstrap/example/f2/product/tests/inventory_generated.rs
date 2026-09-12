#[path = "../src/inventory.rs"]
mod inventory;

use inventory::{Inventory, Product, ProductId, RestockTask};

#[test]
fn req_inventory_1_e8972aee() {
    let sku_1042 = ProductId::from("SKU-1042");
    let sku_2000 = ProductId::from("SKU-2000");
    let inventory = Inventory::from_warehouse_products(vec![
        Product { id: sku_1042.clone(), stock: 12, reorder_point: 5 },
        Product { id: sku_2000.clone(), stock: 0, reorder_point: 3 },
    ]);

    assert_eq!(inventory.stock_for(&sku_1042), Some(12));
    assert_eq!(inventory.stock_for(&sku_2000), Some(0));
}

#[test]
fn req_inventory_2_69ab75c3() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::from_warehouse_products(vec![Product {
        id: sku.clone(),
        stock: 40,
        reorder_point: 10,
    }]);

    let remaining = inventory.pick_for_shipment(&sku, 7).expect("shipment pick should succeed");

    assert_eq!(remaining, 33);
    assert_eq!(inventory.stock_for(&sku), Some(33));
}

#[test]
fn req_inventory_3_900f93cb() {
    let mut inventory = Inventory::default();
    let product = Product {
        id: ProductId::from("SKU-1042"),
        stock: 5,
        reorder_point: 5,
    };

    let task = inventory.evaluate_reorder_point(&product);

    assert_eq!(
        task,
        Some(RestockTask {
            product_id: ProductId::from("SKU-1042"),
            current_stock: 5,
            reorder_point: 5,
        })
    );
    assert_eq!(inventory.restock_tasks().len(), 1);
}

#[test]
fn req_inventory_4_bdea2213() {
    let example = Inventory::typical_day_sku_1042_morning_arrival();

    assert_eq!(example.sku, ProductId::from("SKU-1042"));
    assert_eq!(example.time_of_day, "morning");
    assert_eq!(example.units_arrived, 40);
    assert!(example.stock_after > example.stock_before);
    assert_eq!(example.stock_after, example.stock_before + 40);
}

#[test]
fn req_returns_1_5429692a() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::from_warehouse_products(vec![Product {
        id: sku.clone(),
        stock: 3,
        reorder_point: 2,
    }]);

    let stock_after_return = inventory
        .receive_customer_return("customer-7", sku.clone(), 2)
        .expect("customer return should bring goods back into inventory");

    assert_eq!(stock_after_return, 5);
    assert_eq!(inventory.stock_for(&sku), Some(5));
    assert_eq!(inventory.received_returns()[0].customer_id, "customer-7");
    assert_eq!(inventory.received_returns()[0].product_id, sku);
}

#[test]
fn req_system_7_52b74dfb() {
    let inventory = Inventory::default();

    assert!(inventory.covers_warehouse_side_of_orderly());
}
