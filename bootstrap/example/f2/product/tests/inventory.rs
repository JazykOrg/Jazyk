// tests/inventory.rs

use product::stock::{Inventory, Product, ProductId};

#[test]
fn req_inventory_1_e8972aee() {
    let products = vec![
        Product { id: ProductId(1), stock: 12 },
        Product { id: ProductId(2), stock: 0 },
        Product { id: ProductId(3), stock: 7 },
    ];
    let expected = [(ProductId(1), 12), (ProductId(2), 0), (ProductId(3), 7)];

    let inventory = Inventory::from_warehouse_products(products);

    for (product_id, stock) in expected {
        assert_eq!(
            inventory.stock_for(product_id),
            Some(stock),
            "Inventory must track the Stock for every Product in the warehouse"
        );
    }
}
