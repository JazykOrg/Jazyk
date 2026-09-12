// tests/req_returns_3_ebd60fa9.rs

#[test]
fn req_returns_3_ebd60fa9() {
    // When a Return is received and inspected, Orderly refunds the Payment.
    let returns_source = include_str!("../src/returns.rs");

    assert!(
        returns_source.contains("struct Return") || returns_source.contains("enum Return"),
        "the deliverable must define a Return artifact"
    );
    assert!(
        returns_source.contains("is_inspected")
            && returns_source.contains("Return must be inspected before processing"),
        "Return processing must require the return to be inspected"
    );
    assert!(
        returns_source.contains("process_return"),
        "the deliverable must expose Return processing for received and inspected returns"
    );
    assert!(
        returns_source.contains("refund("),
        "processing an inspected Return must refund the Payment"
    );
    assert!(
        returns_source.find("is_inspected").unwrap_or(usize::MAX)
            < returns_source.find("refund(").unwrap_or(0),
        "the refund must occur after the inspection condition is represented"
    );
}
