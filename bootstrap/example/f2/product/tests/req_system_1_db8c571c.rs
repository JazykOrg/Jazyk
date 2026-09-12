#[test]
fn req_system_1_db8c571c() {
    // req:system-1 [req_system_1_db8c571c]: Orderly is an online shop system.
    // quote: Orderly is an online shop system.
    let manifest = include_str!("../Cargo.toml");
    let lib = include_str!("../src/lib.rs");
    let implementation = format!("{}\n{}", manifest, lib);

    assert!(
        implementation.contains("name = \"orderly\"")
            && lib.contains("pub mod catalog;")
            && lib.contains("pub mod product;")
            && lib.contains("pub mod order;")
            && lib.contains("pub mod payment;")
            && lib.contains("pub mod shipment;")
            && lib.contains("pub mod customer;"),
        "Orderly must identify itself as the orderly package and expose online-shop system modules for catalog, product, order, payment, shipment, and customer"
    );
}
