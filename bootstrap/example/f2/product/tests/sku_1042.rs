#[path = "../src/sku_1042.rs"]
mod sku_1042;

use sku_1042::morning_arrival_raises_stock;

#[test]
// req:req:inventory-4 hash:bdea2213
fn req_inventory_4_bdea2213() {
    let stock_before = 17;
    let example = morning_arrival_raises_stock(stock_before);

    assert_eq!(example.sku, "SKU-1042");
    assert_eq!(example.time_of_day, "morning");
    assert_eq!(example.units_arriving, 40);
    assert_eq!(example.stock_before, stock_before);
    assert!(example.stock_after > example.stock_before);
    assert_eq!(example.stock_after, example.stock_before + 40);
}
