// tests/mobile_app.rs
use product::mobile_app;

#[test]
fn req_roadmap_2_c05cab74() {
    // req:roadmap-2 hash:c05cab74 quote: A mobile app for warehouse picking.
    let app = mobile_app::MobileApp::new();
    assert!(app.get_function().contains("warehouse picking"), "The MobileApp must support warehouse picking.");
}