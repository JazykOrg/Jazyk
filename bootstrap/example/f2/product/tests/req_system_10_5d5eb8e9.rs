#[test]
fn req_system_10_5d5eb8e9() {
    // req:system-10 [req_system_10_5d5eb8e9]: When any component rejects a request, Orderly tells the Customer why.
    // quote: When any component rejects a request, the system shall tell the Customer why.
    let rejection_sources = [
        ("src/admin_cli.rs", include_str!("../src/admin_cli.rs")),
        ("src/customer.rs", include_str!("../src/customer.rs")),
        ("src/payment.rs", include_str!("../src/payment.rs")),
        ("src/product.rs", include_str!("../src/product.rs")),
        ("src/returns.rs", include_str!("../src/returns.rs")),
        ("src/stock.rs", include_str!("../src/stock.rs")),
    ];

    let required_explanations = [
        "Configuration file is missing.",
        "Configuration file is empty.",
        "Quantity must be positive.",
        "Stock reservation failed for one or more products.",
        "Order ID",
        "not found for customer.",
        "Cannot delete account while having unpaid orders.",
        "Payment status is not Confirmed.",
        "Payment deadline exceeded.",
        "Payment was not confirmed or is already refunded.",
        "Insufficient stock to fulfill shipment.",
        "Reservation failed for Product ID",
        "invalid quantity.",
        "Failed to process refund during stock restoration.",
        "Return must be inspected before processing.",
        "Failed to refund payment.",
        "Failed to restore stock.",
        "Order quantity must be positive",
        "Not enough stock to reserve for order",
    ];

    let implementation = rejection_sources
        .iter()
        .map(|(_, source)| *source)
        .collect::<Vec<_>>()
        .join("\n");

    for explanation in required_explanations {
        assert!(
            implementation.contains(explanation),
            "Orderly must preserve the rejection explanation `{}` so a Customer can be told why",
            explanation
        );
    }

    for (path, source) in rejection_sources {
        for (line_number, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if (trimmed.starts_with("return Err(") || trimmed.starts_with("Err("))
                && !trimmed.starts_with("//")
            {
                assert!(
                    trimmed.contains('"') || trimmed.contains("format!("),
                    "{}:{} rejects without an explanatory Customer-facing message: {}",
                    path,
                    line_number + 1,
                    trimmed
                );
                assert!(
                    !trimmed.contains("Err(())"),
                    "{}:{} rejects with an empty unit error instead of telling the Customer why",
                    path,
                    line_number + 1
                );
            }
        }
    }
}
