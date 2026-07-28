// tests/gift_wrapping.rs

use product::gift_wrapping::{CheckoutService, GiftWrappingOption};

#[test]
// req:roadmap-1 hash:157f85bc The system shall support gift wrapping at checkout. quote: Gift wrapping at checkout.
fn test_support_gift_wrapping() {
    let service = CheckoutService::new();

    // Test case 1: Setting a standard wrap option
    let result_standard = service.set_gift_wrapping(GiftWrappingOption::StandardWrap);
    assert_eq!(result_standard, GiftWrappingOption::StandardWrap, "Should successfully set StandardWrap");

    // Test case 2: Setting no wrapping
    let result_none = service.set_gift_wrapping(GiftWrappingOption::None);
    assert_eq!(result_none, GiftWrappingOption::None, "Should successfully set None");

    // Test case 3: Processing a premium wrap request
    let is_processed = service.process_gift_wrapping(101, GiftWrappingOption::PremiumWrap);
    assert!(is_processed, "System must support processing PremiumWrap at checkout");
}