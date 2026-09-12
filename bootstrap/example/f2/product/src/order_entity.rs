use chrono::{DateTime, Duration, Utc};

pub type CustomerId = String;
pub type ProductId = String;
pub type ShipmentId = String;
pub type ReturnId = String;

#[derive(Debug, Clone, PartialEq)]
pub struct ProductSnapshot {
    pub product_id: ProductId,
    pub quantity: u32,
    pub placement_price: f64,
    pub from_catalog: bool,
    pub stock_reserved: bool,
    pub in_stock: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderStatus {
    Submitted,
    Paid,
    OnHold,
    Canceled,
    Shipped,
    Delivered,
    Returned,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceEntry {
    pub status: OrderStatus,
    pub at: DateTime<Utc>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub order_id: String,
    pub customer_id: CustomerId,
    pub items: Vec<ProductSnapshot>,
    pub status: OrderStatus,
    pub placed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub payment_failures: u8,
    pub shipment_id: Option<ShipmentId>,
    pub return_id: Option<ReturnId>,
    pub trace: Vec<TraceEntry>,
    pub notifications: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shipment {
    pub shipment_id: ShipmentId,
    pub order_id: String,
    pub buyer_id: CustomerId,
    pub packed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Return {
    pub return_id: ReturnId,
    pub order_id: String,
    pub customer_id: CustomerId,
    pub opened_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub struct AdminCli {
    orders: Vec<Order>,
}

impl AdminCli {
    // req:req:admin-6 hash:2c3c595c
    pub fn with_orders(orders: Vec<Order>) -> Self {
        Self { orders }
    }

    pub fn report(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<Order> {
        self.orders
            .iter()
            .filter(|order| order.placed_at >= start && order.placed_at <= end)
            .cloned()
            .collect()
    }
}

impl Order {
    // req:req:orders-1 hash:b4add94c
    // req:req:orders-2 hash:bfac24ba
    // req:req:system-3 hash:77cf84bb
    pub fn place(order_id: impl Into<String>, customer_id: impl Into<String>, mut items: Vec<ProductSnapshot>, placed_at: DateTime<Utc>) -> Self {
        assert!(!items.is_empty(), "an Order contains one or more Products from the Catalog");
        for item in &mut items {
            assert!(item.from_catalog, "each Product in an Order must come from the Catalog");
        }
        let mut order = Self {
            order_id: order_id.into(),
            customer_id: customer_id.into(),
            items,
            status: OrderStatus::Submitted,
            placed_at,
            updated_at: placed_at,
            payment_failures: 0,
            shipment_id: None,
            return_id: None,
            trace: Vec::new(),
            notifications: Vec::new(),
        };
        order.trace.push(TraceEntry { status: OrderStatus::Submitted, at: placed_at, note: "placement".to_string() });
        order
    }

    fn transition(&mut self, status: OrderStatus, at: DateTime<Utc>, note: &str) {
        self.status = status.clone();
        self.updated_at = at;
        self.trace.push(TraceEntry { status, at, note: note.to_string() });
        self.notifications.push(format!("email:{}:{}", self.customer_id, note));
    }

    // req:req:orders-3 hash:ebd76a10
    pub fn submit_and_reserve_stock(&mut self) {
        for item in &mut self.items {
            item.stock_reserved = true;
        }
    }

    // req:req:orders-4 hash:29f2c263
    pub fn cancel_if_unpaid_after_21_days(&mut self, now: DateTime<Utc>) -> bool {
        let unpaid = !matches!(self.status, OrderStatus::Paid | OrderStatus::Shipped | OrderStatus::Delivered | OrderStatus::Returned);
        if unpaid && now > self.placed_at + Duration::days(21) {
            self.transition(OrderStatus::Canceled, now, "cancelled for unpaid order after 21 days");
            true
        } else {
            false
        }
    }

    // req:req:orders-5 hash:9a77aa9a
    pub fn listed_products(&self) -> Vec<(ProductId, u32, f64)> {
        self.items.iter().map(|item| (item.product_id.clone(), item.quantity, item.placement_price)).collect()
    }

    // req:req:payment-1 hash:48c13b8e
    // req:req:payment-2 hash:2f52f7b4
    pub fn confirm_payment(&mut self, payment_id: &str, at: DateTime<Utc>) {
        self.transition(OrderStatus::Paid, at, &format!("payment {payment_id} confirmed and settled order"));
    }

    // req:req:payment-3 hash:2e4285d5
    pub fn is_within_30_day_payment_window(&self, now: DateTime<Utc>) -> bool {
        self.status == OrderStatus::Paid || now <= self.placed_at + Duration::days(30)
    }

    // req:req:payment-4 hash:6f3f1877
    pub fn record_failed_payment(&mut self, at: DateTime<Utc>) {
        self.payment_failures += 1;
        if self.payment_failures >= 3 {
            self.transition(OrderStatus::OnHold, at, "payment failed three times");
        }
    }

    // req:req:shipping-2 hash:58de09dc
    pub fn create_shipment_if_paid_and_in_stock(&mut self, shipment_id: impl Into<String>, at: DateTime<Utc>) -> Option<Shipment> {
        if self.status == OrderStatus::Paid && self.items.iter().all(|item| item.in_stock) {
            let shipment_id = shipment_id.into();
            self.shipment_id = Some(shipment_id.clone());
            self.transition(OrderStatus::Shipped, at, "shipment created for paid in-stock order");
            Some(Shipment { shipment_id, order_id: self.order_id.clone(), buyer_id: self.customer_id.clone(), packed: true })
        } else {
            None
        }
    }

    // req:req:shipping-1 hash:f1081307
    pub fn shipment_delivers_packed_order_to_buyer(&self, shipment: &Shipment) -> bool {
        shipment.order_id == self.order_id && shipment.buyer_id == self.customer_id && shipment.packed
    }

    pub fn mark_delivered(&mut self, at: DateTime<Utc>) {
        self.transition(OrderStatus::Delivered, at, "delivery");
    }

    // req:req:returns-2 hash:51943b2e
    pub fn open_return_within_30_days(&mut self, return_id: impl Into<String>, now: DateTime<Utc>) -> Option<Return> {
        if self.status == OrderStatus::Delivered && now <= self.updated_at + Duration::days(30) {
            let return_id = return_id.into();
            self.return_id = Some(return_id.clone());
            let opened = Return { return_id, order_id: self.order_id.clone(), customer_id: self.customer_id.clone(), opened_at: now };
            self.transition(OrderStatus::Returned, now, "return opened within 30 days of delivery");
            Some(opened)
        } else {
            None
        }
    }

    // req:req:customer-4 hash:7f25c49a
    pub fn customer_can_delete_account(&self) -> bool {
        !matches!(self.status, OrderStatus::Submitted | OrderStatus::OnHold)
    }

    // req:req:customer-5 hash:91839be5
    pub fn email_notifications(&self) -> &[String] {
        &self.notifications
    }

    // req:req:system-9 hash:012251dd
    pub fn trace_from_placement_to_delivery_or_return(&self) -> &[TraceEntry] {
        &self.trace
    }
}
