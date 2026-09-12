#[path = "../src/sku.rs"]
mod sku;
#[path = "../src/inventory.rs"]
mod inventory;

use inventory::{Inventory, ProductId};
use sku::SellableProductVariant;

// req:req:glossary-1 hash:3e21216a
#[test]
fn req_glossary_1_3e21216a() {
    let variant = SellableProductVariant::with_warehouse_label(1042, "Variant A", "SKU-1042");

    assert_eq!(variant.product_id, 1042);
    assert_eq!(variant.variant_name, "Variant A");
    assert_eq!(variant.warehouse_label.as_str(), "SKU-1042");
}

// req:req:inventory-4 hash:bdea2213
#[test]
fn req_inventory_4_bdea2213() {
    let example = Inventory::typical_day_sku_1042_morning_arrival();

    assert_eq!(example.sku, ProductId::from("SKU-1042"));
    assert_eq!(example.time_of_day, "morning");
    assert_eq!(example.units_arrived, 40);
    assert!(example.stock_after > example.stock_before);
    assert_eq!(example.stock_after, example.stock_before + 40);
}
