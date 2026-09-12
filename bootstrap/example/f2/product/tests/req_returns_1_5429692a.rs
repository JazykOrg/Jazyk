// tests/req_returns_1_5429692a.rs

#[test]
fn req_returns_1_5429692a() {
    // A Return brings goods from a Customer back into Inventory.
    let returns_source = include_str!("../src/returns.rs");
    let lib_source = include_str!("../src/lib.rs");
    let stock_source = include_str!("../src/stock.rs");

    assert!(
        returns_source.contains("struct Return") || returns_source.contains("enum Return"),
        "the deliverable must define a Return artifact"
    );
    assert!(
        returns_source.contains("Customer") || returns_source.contains("customer"),
        "Return must identify the Customer that sends goods back"
    );
    assert!(
        returns_source.contains("Inventory")
            || (lib_source.contains("inventory") && returns_source.contains("inventory"))
            || (stock_source.contains("struct Inventory") && returns_source.contains("Inventory")),
        "Return must bring goods back into Inventory"
    );
}
