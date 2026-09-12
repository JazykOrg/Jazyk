// src/account.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountOrder {
    pub id: u32,
    pub paid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub customer_name: String,
    pub customer_kind: CustomerKind,
    pub provider: String,
    pub orders: Vec<AccountOrder>,
    deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomerKind {
    Person,
    Organization,
}

impl Account {
    pub fn held_by_customer_with_orderly(customer_name: impl Into<String>, customer_kind: CustomerKind) -> Self {
        Self {
            customer_name: customer_name.into(),
            customer_kind,
            provider: "Orderly".to_string(),
            orders: Vec::new(),
            deleted: false,
        }
    }

    pub fn add_order(&mut self, id: u32, paid: bool) {
        self.orders.push(AccountOrder { id, paid });
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    pub fn delete(&mut self) -> Result<(), String> {
        if self.orders.iter().any(|order| !order.paid) {
            return Err("Customer cannot delete their account while an Order is unpaid.".to_string());
        }
        self.deleted = true;
        Ok(())
    }
}
