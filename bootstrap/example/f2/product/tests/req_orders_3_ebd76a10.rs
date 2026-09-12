use product::stock::{Order, OrderItem, StockManager};

#[test]
fn req_orders_3_ebd76a10_reserves_stock_for_each_product_when_customer_submits_order() {
    let manager = StockManager::new();
    let mut order = Order {
        id: 103,
        items: vec![
            OrderItem { product_id: 1, quantity: 2 },
            OrderItem { product_id: 2, quantity: 1 },
        ],
    };

    let result = manager.reserve_stock(&mut order);

    assert!(
        result.is_ok(),
        "submitting an order must reserve stock for every product in the order"
    );
}
