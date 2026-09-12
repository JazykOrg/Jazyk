#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: String,
    pub name: String,
    pub in_stock: bool,
}

impl Product {
    pub fn new(id: impl Into<String>, name: impl Into<String>, in_stock: bool) -> Self {
        Self { id: id.into(), name: name.into(), in_stock }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    products: Vec<Product>,
}

impl Catalog {
    pub fn new(products: Vec<Product>) -> Self {
        Self { products }
    }

    // req:req:catalog-1 hash:2dd1e729
    pub fn lists_every_product_orderly_sells(&self) -> &[Product] {
        &self.products
    }

    // req:req:catalog-4 hash:72d3d04b
    pub fn visible_products(&self) -> Vec<Product> {
        self.products.iter().filter(|product| product.in_stock).cloned().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customer {
    pub id: String,
    pub kind: CustomerKind,
    pub holds_orderly_account: bool,
    pub email: String,
    pub notifications: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomerKind {
    Person,
    Organization,
}

impl Customer {
    // req:req:customer-1 hash:f180c28a
    pub fn person_or_organization_with_account(id: impl Into<String>, kind: CustomerKind, email: impl Into<String>) -> Self {
        Self { id: id.into(), kind, holds_orderly_account: true, email: email.into(), notifications: Vec::new() }
    }

    // req:req:customer-5 hash:91839be5
    pub fn notify_by_email_for_order_state_change(&mut self, order_id: &str, new_state: OrderState) {
        self.notifications.push(format!("Email to {}: Order {} changed state to {:?}", self.email, order_id, new_state));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockLine {
    pub product_id: String,
    pub quantity: u32,
    pub reserved: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub stock: Vec<StockLine>,
}

impl Inventory {
    pub fn new(stock: Vec<StockLine>) -> Self {
        Self { stock }
    }

    // req:req:system-7 hash:52b74dfb
    pub fn covers_warehouse_side(&self) -> &'static str {
        "Inventory covers the warehouse side of Orderly"
    }

    pub fn has_every_product_in_stock(&self, products: &[OrderLine]) -> bool {
        products.iter().all(|line| self.stock.iter().any(|stock| stock.product_id == line.product_id && stock.quantity >= line.quantity))
    }

    pub fn reserve(&mut self, product_id: &str, quantity: u32) -> bool {
        if let Some(stock) = self.stock.iter_mut().find(|stock| stock.product_id == product_id && stock.quantity >= quantity) {
            stock.quantity -= quantity;
            stock.reserved += quantity;
            true
        } else {
            false
        }
    }

    pub fn restore(&mut self, product_id: &str, quantity: u32) -> bool {
        if let Some(stock) = self.stock.iter_mut().find(|stock| stock.product_id == product_id) {
            stock.quantity += quantity;
            true
        } else {
            self.stock.push(StockLine { product_id: product_id.to_string(), quantity, reserved: 0 });
            true
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLine {
    pub product_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderState {
    Placed,
    Paid,
    OnHold,
    Canceled,
    Shipped,
    Delivered,
    Returned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    pub state: OrderState,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: String,
    pub customer_id: String,
    pub lines: Vec<OrderLine>,
    pub state: OrderState,
    pub days_since_placement: u32,
    pub trace: Vec<TraceEntry>,
}

impl Order {
    // req:req:system-3 hash:77cf84bb
    pub fn placed_in_orderly(id: impl Into<String>, customer_id: impl Into<String>, lines: Vec<OrderLine>) -> Self {
        let mut order = Self { id: id.into(), customer_id: customer_id.into(), lines, state: OrderState::Placed, days_since_placement: 0, trace: Vec::new() };
        order.trace.push(TraceEntry { state: OrderState::Placed, note: "placement".to_string() });
        order
    }

    // req:req:orders-3 hash:ebd76a10
    pub fn submit_and_reserve_stock(&mut self, inventory: &mut Inventory) -> bool {
        let can_reserve_all = self.lines.iter().all(|line| inventory.stock.iter().any(|stock| stock.product_id == line.product_id && stock.quantity >= line.quantity));
        if can_reserve_all {
            for line in &self.lines {
                inventory.reserve(&line.product_id, line.quantity);
            }
        }
        can_reserve_all
    }

    // req:req:orders-4 hash:29f2c263
    pub fn cancel_if_not_paid_within_21_days(&mut self) -> bool {
        if self.state != OrderState::Paid && self.days_since_placement > 21 {
            self.state = OrderState::Canceled;
            self.trace.push(TraceEntry { state: OrderState::Canceled, note: "not paid within 21 days of placement".to_string() });
            true
        } else {
            false
        }
    }

    // req:req:payment-2 hash:2f52f7b4
    pub fn mark_paid_when_payment_confirmed(&mut self, payment: &Payment) -> bool {
        if payment.order_id == self.id && payment.status == PaymentStatus::Confirmed {
            self.state = OrderState::Paid;
            self.trace.push(TraceEntry { state: OrderState::Paid, note: "Payment confirmed".to_string() });
            true
        } else {
            false
        }
    }

    // req:req:payment-4 hash:6f3f1877
    pub fn put_on_hold_after_three_payment_failures(&mut self, payment: &Payment) -> bool {
        if payment.order_id == self.id && payment.failed_attempts >= 3 {
            self.state = OrderState::OnHold;
            self.trace.push(TraceEntry { state: OrderState::OnHold, note: "Payment failed three times".to_string() });
            true
        } else {
            false
        }
    }

    // req:req:shipping-2 hash:58de09dc
    pub fn create_shipment_when_paid_and_in_stock(&mut self, inventory: &Inventory, shipment_id: impl Into<String>) -> Option<Shipment> {
        if self.state == OrderState::Paid && inventory.has_every_product_in_stock(&self.lines) {
            let shipment = Shipment { id: shipment_id.into(), order_id: self.id.clone(), buyer_id: self.customer_id.clone(), state: ShipmentState::Created, tracking_links: Vec::new(), delivery_attempts: 0, buyer_refunded: false };
            self.state = OrderState::Shipped;
            self.trace.push(TraceEntry { state: OrderState::Shipped, note: "Shipment created".to_string() });
            Some(shipment)
        } else {
            None
        }
    }

    // req:req:system-9 hash:012251dd
    pub fn traceable_from_placement_to_delivery_or_return(&self) -> bool {
        self.trace.first().map(|entry| entry.state == OrderState::Placed).unwrap_or(false)
            && self.trace.iter().any(|entry| matches!(entry.state, OrderState::Delivered | OrderState::Returned | OrderState::Shipped | OrderState::Canceled))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentStatus {
    Pending,
    Confirmed,
    Failed,
    Refunded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    pub id: String,
    pub order_id: String,
    pub status: PaymentStatus,
    pub failed_attempts: u8,
}

impl Payment {
    pub fn new(id: impl Into<String>, order_id: impl Into<String>) -> Self {
        Self { id: id.into(), order_id: order_id.into(), status: PaymentStatus::Pending, failed_attempts: 0 }
    }

    pub fn confirm(&mut self) {
        self.status = PaymentStatus::Confirmed;
    }

    pub fn fail(&mut self) {
        self.status = PaymentStatus::Failed;
        self.failed_attempts = self.failed_attempts.saturating_add(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShipmentState {
    Created,
    LeftWarehouse,
    ReturnedToWarehouse,
    Delivered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shipment {
    pub id: String,
    pub order_id: String,
    pub buyer_id: String,
    pub state: ShipmentState,
    pub tracking_links: Vec<String>,
    pub delivery_attempts: u8,
    pub buyer_refunded: bool,
}

impl Shipment {
    // req:req:shipping-3 hash:0f6550ac
    pub fn leave_warehouse_and_send_tracking_link(&mut self, tracking_link: impl Into<String>) -> String {
        let tracking_link = tracking_link.into();
        self.state = ShipmentState::LeftWarehouse;
        self.tracking_links.push(tracking_link.clone());
        tracking_link
    }

    pub fn record_failed_delivery_attempt(&mut self) {
        self.delivery_attempts = self.delivery_attempts.saturating_add(1);
    }

    // req:req:shipping-4 hash:0cab498b
    pub fn return_to_warehouse_after_two_attempts(&mut self) -> bool {
        if self.delivery_attempts >= 2 {
            self.state = ShipmentState::ReturnedToWarehouse;
            true
        } else {
            false
        }
    }

    // req:req:shipping-5 hash:19fe0ed0
    pub fn refund_buyer_after_two_attempts(&mut self) -> bool {
        if self.delivery_attempts >= 2 {
            self.buyer_refunded = true;
            true
        } else {
            false
        }
    }

    // req:req:system-5 hash:2cf777e3
    pub fn received_by_customer(&mut self, customer_id: &str) -> bool {
        if self.buyer_id == customer_id {
            self.state = ShipmentState::Delivered;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    pub id: String,
    pub order_id: String,
    pub received: bool,
    pub inspected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundReceipt {
    pub payment_id: String,
    pub buyer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orderly {
    pub catalog: Catalog,
    pub inventory: Inventory,
    pub admin_cli_tool: &'static str,
    pub rejection_reasons: Vec<String>,
}

impl Orderly {
    // req:req:system-1 hash:db8c571c
    pub fn online_shop_system(catalog: Catalog, inventory: Inventory) -> Self {
        Self { catalog, inventory, admin_cli_tool: "Admin CLI", rejection_reasons: Vec::new() }
    }

    // req:req:admin-1 hash:e8928a4d
    pub fn admin_cli_is_operator_tool(&self) -> &'static str {
        self.admin_cli_tool
    }

    // req:req:admin-2 hash:1683a24d
    pub fn start_command() -> &'static str {
        "orderly serve --port 8080 --config /etc/orderly/config.toml"
    }

    // req:req:system-2 hash:b40be817
    pub fn customer_browses_catalog(&self, _customer: &Customer) -> Vec<Product> {
        self.catalog.visible_products()
    }

    // req:req:system-4 hash:4e85ad93
    pub fn customer_pays_through_payment(&self, order: &Order) -> Payment {
        Payment::new("payment", order.id.clone())
    }

    // req:req:system-6 hash:f1885d6c
    pub fn unwanted_goods_go_back_through_returns(&self, order: &Order) -> Return {
        Return { id: format!("return-for-{}", order.id), order_id: order.id.clone(), received: false, inspected: false }
    }

    // req:req:system-8 hash:9d522f0b
    pub fn operators_manage_with_admin_cli(&self) -> bool {
        self.admin_cli_tool == "Admin CLI"
    }

    // req:req:system-10 hash:5d5eb8e9
    pub fn reject_request_with_customer_reason<T>(&mut self, reason: impl Into<String>) -> Result<T, String> {
        let reason = reason.into();
        self.rejection_reasons.push(reason.clone());
        Err(reason)
    }
}

// req:req:payment-5 hash:2793ad62
pub fn notify_customer_after_three_payment_failures(customer: &mut Customer, payment: &Payment) -> bool {
    if payment.failed_attempts >= 3 {
        customer.notifications.push(format!("Email to {}: Payment failed three times", customer.email));
        true
    } else {
        false
    }
}

// req:req:returns-3 hash:ebd60fa9
pub fn refund_payment_when_return_received_and_inspected(return_request: &Return, payment: &mut Payment) -> bool {
    if return_request.received && return_request.inspected {
        payment.status = PaymentStatus::Refunded;
        true
    } else {
        false
    }
}

// req:req:returns-4 hash:264fa475
pub fn restore_stock_when_return_received_and_inspected(return_request: &Return, inventory: &mut Inventory, line: &OrderLine) -> bool {
    if return_request.received && return_request.inspected {
        inventory.restore(&line.product_id, line.quantity)
    } else {
        false
    }
}
