use product::catalog::{Catalog, Product};

#[test]
// req:req:system-2 hash:b40be817
fn req_system_2_b40be817() {
    let mut catalog = Catalog::new();
    let visible = Product::new(1, "Visible product", 8);
    let hidden = Product::new(2, "Hidden product", 8);
    catalog.add_product(visible.clone(), 5);
    catalog.add_product(hidden, 0);

    let browsed = catalog.browse_catalog("customer-1");

    assert_eq!(browsed, vec![&visible]);
}
