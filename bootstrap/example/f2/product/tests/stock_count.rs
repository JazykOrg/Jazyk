#[path = "../src/stock_count.rs"]
mod stock_count;

use stock_count::{Catalog, InspectedReturn, Inventory, Order, OrderItem, Product, ProductId, ReorderPoint, ShipmentPick};

fn product(id: &str, stock: u32, reorder_point: u32) -> Product {
    Product::new(ProductId::from(id), format!("Product {id}"), stock, reorder_point)
}

#[test]
// req:req:catalog-4 hash:72d3d04b
fn req_catalog_4_72d3d04b() {
    let in_stock = product("SKU-1", 3, 1);
    let out_of_stock = product("SKU-2", 0, 1);
    let mut inventory = Inventory::default();
    inventory.track_product(in_stock.clone());
    inventory.track_product(out_of_stock.clone());
    let mut catalog = Catalog::default();
    catalog.add_product(in_stock.clone());
    catalog.add_product(out_of_stock);

    let visible = catalog.visible_products_in_stock(&inventory);

    assert_eq!(visible, vec![&in_stock]);
}

#[test]
// req:req:catalog-5 hash:84eddf06
fn req_catalog_5_84eddf06() {
    let id = ProductId::from("SKU-zero");
    let mut inventory = Inventory::default();
    let item = Product::new(id.clone(), "Zero stock", 1, 1);
    inventory.track_product(item.clone());
    let mut catalog = Catalog::default();
    catalog.add_product(item);

    inventory
        .pick_for_shipment(ShipmentPick { product_id: id.clone(), picked_quantity: 1 })
        .expect("pick reaches zero");

    assert!(catalog.hides_product_when_stock_reaches_zero(&id, &inventory));
}

#[test]
// req:req:glossary-3 hash:0d257092
fn req_glossary_3_0d257092() {
    assert_eq!(ReorderPoint::DEFINITION, "Reorder point: the Stock level that triggers restocking.");
    assert!(ReorderPoint::triggers_restocking(5, 5));
    assert!(!ReorderPoint::triggers_restocking(6, 5));
}

#[test]
// req:req:inventory-1 hash:e8972aee
fn req_inventory_1_e8972aee() {
    let sku_1042 = ProductId::from("SKU-1042");
    let sku_2000 = ProductId::from("SKU-2000");
    let mut inventory = Inventory::default();

    inventory.track_product(product("SKU-1042", 12, 5));
    inventory.track_product(product("SKU-2000", 0, 3));

    assert_eq!(inventory.stock_for(&sku_1042), Some(12));
    assert_eq!(inventory.stock_for(&sku_2000), Some(0));
}

#[test]
// req:req:inventory-2 hash:69ab75c3
fn req_inventory_2_69ab75c3() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::default();
    inventory.track_product(product("SKU-1042", 40, 10));

    let remaining = inventory
        .pick_for_shipment(ShipmentPick { product_id: sku.clone(), picked_quantity: 7 })
        .expect("shipment pick should succeed");

    assert_eq!(remaining, 33);
    assert_eq!(inventory.stock_for(&sku), Some(33));
}

#[test]
// req:req:inventory-3 hash:900f93cb
fn req_inventory_3_900f93cb() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::default();
    inventory.track_product(product("SKU-1042", 5, 5));

    let task = inventory.create_restock_task_if_reorder_point_reached(&sku);

    assert!(task.is_some());
    assert_eq!(inventory.restock_tasks().len(), 1);
    assert_eq!(inventory.restock_tasks()[0].current_stock, 5);
}

#[test]
// req:req:inventory-4 hash:bdea2213
fn req_inventory_4_bdea2213() {
    let example = Inventory::typical_day_sku_1042_morning_arrival();

    assert_eq!(example.sku, ProductId::from("SKU-1042"));
    assert_eq!(example.time_of_day, "morning");
    assert_eq!(example.units_arrived, 40);
    assert!(example.stock_after > example.stock_before);
    assert_eq!(example.stock_after, example.stock_before + 40);
}

#[test]
// req:req:inventory-5 hash:4a774f90
fn req_inventory_5_4a774f90() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::default();
    inventory.track_product(product("SKU-1042", 5, 2));

    let result = inventory.decrease_without_negative_stock(&sku, 6);

    assert!(result.is_err());
    assert_eq!(inventory.stock_for(&sku), Some(5));
}

#[test]
// req:req:orders-3 hash:ebd76a10
fn req_orders_3_ebd76a10() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::default();
    inventory.track_product(product("SKU-1042", 8, 2));
    let mut order = Order {
        id: "O-1".to_string(),
        paid: false,
        items: vec![OrderItem { product_id: sku.clone(), quantity: 3, stock_reserved: false }],
    };

    inventory.reserve_stock_for_order(&mut order).expect("stock reservation should succeed");

    assert!(order.items.iter().all(|item| item.stock_reserved));
    assert_eq!(inventory.stock_for(&sku), Some(5));
}

#[test]
// req:req:returns-4 hash:264fa475
fn req_returns_4_264fa475() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::default();
    inventory.track_product(product("SKU-1042", 2, 1));
    let returned = InspectedReturn { product_id: sku.clone(), quantity: 4, received: true, inspected: true };

    let restored = inventory
        .restore_stock_from_received_inspected_return(&returned)
        .expect("received and inspected return restores stock");

    assert_eq!(restored, 6);
    assert_eq!(inventory.stock_for(&sku), Some(6));
}

#[test]
// req:req:shipping-2 hash:58de09dc
fn req_shipping_2_58de09dc() {
    let sku = ProductId::from("SKU-1042");
    let mut inventory = Inventory::default();
    inventory.track_product(product("SKU-1042", 4, 1));
    let paid_order = Order {
        id: "O-paid".to_string(),
        paid: true,
        items: vec![OrderItem { product_id: sku, quantity: 2, stock_reserved: false }],
    };

    let shipment = inventory.create_shipment_when_paid_and_in_stock(&paid_order);

    assert!(shipment.is_some());
    assert_eq!(shipment.unwrap().order_id, "O-paid");
}
