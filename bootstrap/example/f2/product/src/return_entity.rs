use chrono::{DateTime, Duration, Utc};

pub type CustomerId = String;
pub type OrderId = String;
pub type ProductId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnLine {
    pub product_id: ProductId,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderDelivery {
    pub order_id: OrderId,
    pub customer_id: CustomerId,
    pub delivered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    pub return_id: String,
    pub customer_id: CustomerId,
    pub order_id: OrderId,
    pub opened_at: DateTime<Utc>,
    pub received: bool,
    pub inspected: bool,
    pub lines: Vec<ReturnLine>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InventoryLedger {
    pub restored: Vec<ReturnLine>,
}

impl InventoryLedger {
    pub fn restore(&mut self, line: ReturnLine) {
        self.restored.push(line);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PaymentLedger {
    pub refunded_orders: Vec<OrderId>,
}

impl PaymentLedger {
    pub fn refund(&mut self, order_id: OrderId) {
        self.refunded_orders.push(order_id);
    }
}

impl Return {
    // req:req:returns-1 hash:5429692a
    pub fn brings_goods_from_customer_back_into_inventory(
        return_id: impl Into<String>,
        customer_id: impl Into<CustomerId>,
        order_id: impl Into<OrderId>,
        opened_at: DateTime<Utc>,
        lines: Vec<ReturnLine>,
    ) -> Self {
        assert!(!lines.is_empty(), "a Return must carry goods back into Inventory");
        Self {
            return_id: return_id.into(),
            customer_id: customer_id.into(),
            order_id: order_id.into(),
            opened_at,
            received: false,
            inspected: false,
            lines,
        }
    }

    // req:req:returns-2 hash:51943b2e
    pub fn open_for_order_within_30_days(
        return_id: impl Into<String>,
        delivery: &OrderDelivery,
        opened_at: DateTime<Utc>,
        lines: Vec<ReturnLine>,
    ) -> Option<Self> {
        if opened_at <= delivery.delivered_at + Duration::days(30) {
            Some(Self::brings_goods_from_customer_back_into_inventory(
                return_id,
                delivery.customer_id.clone(),
                delivery.order_id.clone(),
                opened_at,
                lines,
            ))
        } else {
            None
        }
    }

    pub fn mark_received_and_inspected(&mut self) {
        self.received = true;
        self.inspected = true;
    }

    // req:req:returns-3 hash:ebd60fa9
    pub fn refund_payment_after_received_and_inspected(
        &self,
        payments: &mut PaymentLedger,
    ) -> Result<(), String> {
        if !(self.received && self.inspected) {
            return Err("Return must be received and inspected before Payment is refunded".to_string());
        }
        payments.refund(self.order_id.clone());
        Ok(())
    }

    // req:req:returns-4 hash:264fa475
    pub fn restore_stock_after_received_and_inspected(
        &self,
        inventory: &mut InventoryLedger,
    ) -> Result<(), String> {
        if !(self.received && self.inspected) {
            return Err("Return must be received and inspected before Stock is restored".to_string());
        }
        for line in &self.lines {
            inventory.restore(line.clone());
        }
        Ok(())
    }

    pub fn process_received_inspected(
        &self,
        payments: &mut PaymentLedger,
        inventory: &mut InventoryLedger,
    ) -> Result<(), String> {
        self.refund_payment_after_received_and_inspected(payments)?;
        self.restore_stock_after_received_and_inspected(inventory)
    }
}
