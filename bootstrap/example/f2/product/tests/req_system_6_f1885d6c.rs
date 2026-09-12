#[test]
fn req_system_6_f1885d6c() {
    // req:system-6 [req_system_6_f1885d6c]: Unwanted goods go back through Returns in Orderly.
    // quote: Unwanted goods go back through [Returns](./returns.md).
    let lib_source = include_str!("../src/lib.rs");
    let returns_source = include_str!("../src/returns.rs");

    let uncommented_returns = returns_source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();

    assert!(
        lib_source.contains("pub mod returns"),
        "Orderly must expose the Returns component used when unwanted goods go back"
    );
    assert!(
        returns_source.contains("struct Return"),
        "Orderly must define a Return record for goods going back through Returns"
    );
    assert!(
        uncommented_returns.contains("item_id") && uncommented_returns.contains("quantity"),
        "Returns must carry the unwanted goods being sent back, not only an empty marker"
    );
    assert!(
        uncommented_returns.contains("process_return"),
        "Returns must provide a path that processes goods going back through Returns"
    );
}
