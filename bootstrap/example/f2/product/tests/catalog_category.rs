#[path = "../src/catalog_category.rs"]
mod catalog_category;

use catalog_category::{CatalogCategory, ProductCatalogMembership};

#[test]
fn req_catalog_3_938efd1e() {
    let category = CatalogCategory {
        id: "kitchen".to_string(),
    };
    let membership = ProductCatalogMembership::new("product-1", category.clone());

    assert_eq!(membership.product_id, "product-1");
    assert_eq!(membership.category(), &category);
}
