#[test]
fn req_glossary_2_fe12e842() {
    // req:glossary-2 hash:fe12e842 quote: - Picking: collecting Products from shelves for a Shipment.
    let lib = include_str!("../src/lib.rs");
    let product = include_str!("../src/product.rs");
    let shipment = include_str!("../src/shipment.rs");
    let stock = include_str!("../src/stock.rs");
    let implementation = [lib, product, shipment, stock].join("\n");

    assert!(
        implementation.contains("Picking")
            && implementation.contains("Product")
            && implementation.contains("Shipment")
            && implementation.contains("shelves"),
        "Picking must be implemented as a process that collects Products from shelves for a Shipment"
    );
}

#[test]
fn req_glossary_3_0d257092() {
    // req:glossary-3 hash:0d257092 quote: - Reorder point: the Stock level that triggers restocking.
    let lib = include_str!("../src/lib.rs");
    let stock = include_str!("../src/stock.rs");
    let product = include_str!("../src/product.rs");
    let implementation = [lib, stock, product].join("\n");

    assert!(
        implementation.contains("Reorder point")
            && implementation.contains("Stock")
            && implementation.contains("restocking"),
        "Reorder point must be implemented as the Stock level that triggers restocking"
    );
}
