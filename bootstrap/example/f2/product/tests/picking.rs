#[path = "../src/picking.rs"]
mod picking;

use picking::Picking;

#[test]
fn req_glossary_2_fe12e842() {
    let picking = Picking::collect_products_from_shelves_for_shipment(42, 7, 3)
        .expect("positive product quantity should be picked");

    assert_eq!(picking.product_id, 42);
    assert_eq!(picking.shipment_id, 7);
    assert_eq!(picking.quantity, 3);
    assert_eq!(picking.source, "shelves");
    assert!(picking.collects_product_for_shipment(42, 7));

    assert!(Picking::collect_products_from_shelves_for_shipment(42, 7, 0).is_err());
}
