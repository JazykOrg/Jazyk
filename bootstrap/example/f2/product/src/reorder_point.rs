// src/reorder_point.rs

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductId(pub String);

impl From<&str> for ProductId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductStock {
    pub product_id: ProductId,
    pub stock: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderPoint {
    pub stock_level: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestockTask {
    pub product_id: ProductId,
    pub current_stock: i32,
    pub reorder_point: i32,
}

impl ReorderPoint {
    pub const DEFINITION: &'static str = "Reorder point: the Stock level that triggers restocking.";

    pub fn is_stock_level_that_triggers_restocking(&self, stock: &ProductStock) -> bool {
        stock.stock <= self.stock_level
    }

    pub fn create_restock_task_when_reached(&self, stock: &ProductStock) -> Option<RestockTask> {
        if self.is_stock_level_that_triggers_restocking(stock) {
            Some(RestockTask {
                product_id: stock.product_id.clone(),
                current_stock: stock.stock,
                reorder_point: self.stock_level,
            })
        } else {
            None
        }
    }
}
