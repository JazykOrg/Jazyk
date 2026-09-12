use product::stock::{Inventory, Product, ProductId};

#[test]
fn req_system_7_52b74dfb() {
    let inventory = Inventory::from_warehouse_products(vec![Product { id: ProductId(1), stock: 5 }]);

    assert!(
        inventory.covers_warehouse_side(),
        "Inventory must cover Orderly's warehouse side"
    );
    assert_eq!(
        inventory.stock_for(ProductId(1)),
        Some(5),
        "the warehouse-side Inventory must expose tracked warehouse stock"
    );
}
