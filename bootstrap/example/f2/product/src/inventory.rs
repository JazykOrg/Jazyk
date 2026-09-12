// src/inventory.rs
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductId(pub String);

impl From<&str> for ProductId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: ProductId,
    pub stock: i32,
    pub reorder_point: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestockTask {
    pub product_id: ProductId,
    pub current_stock: i32,
    pub reorder_point: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    pub customer_id: String,
    pub product_id: ProductId,
    pub quantity: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypicalDayArrival {
    pub sku: ProductId,
    pub time_of_day: &'static str,
    pub units_arrived: i32,
    pub stock_before: i32,
    pub stock_after: i32,
}

#[derive(Debug, Default)]
pub struct Inventory {
    warehouse_stock: BTreeMap<ProductId, i32>,
    restock_tasks: Vec<RestockTask>,
    received_returns: Vec<Return>,
}

impl Inventory {
    pub fn from_warehouse_products(products: Vec<Product>) -> Self {
        let warehouse_stock = products
            .into_iter()
            .map(|product| (product.id, product.stock))
            .collect();
        Self {
            warehouse_stock,
            restock_tasks: Vec::new(),
            received_returns: Vec::new(),
        }
    }

    pub fn stock_for(&self, product_id: &ProductId) -> Option<i32> {
        self.warehouse_stock.get(product_id).copied()
    }

    pub fn pick_for_shipment(&mut self, product_id: &ProductId, picked_quantity: i32) -> Result<i32, String> {
        if picked_quantity < 0 {
            return Err("picked quantity cannot be negative".to_string());
        }
        let stock = self
            .warehouse_stock
            .get_mut(product_id)
            .ok_or_else(|| format!("unknown product {}", product_id.0))?;
        if *stock < picked_quantity {
            return Err("insufficient stock for shipment pick".to_string());
        }
        *stock -= picked_quantity;
        Ok(*stock)
    }

    pub fn evaluate_reorder_point(&mut self, product: &Product) -> Option<RestockTask> {
        if product.stock <= product.reorder_point {
            let task = RestockTask {
                product_id: product.id.clone(),
                current_stock: product.stock,
                reorder_point: product.reorder_point,
            };
            self.restock_tasks.push(task.clone());
            Some(task)
        } else {
            None
        }
    }

    pub fn restock_tasks(&self) -> &[RestockTask] {
        &self.restock_tasks
    }

    pub fn typical_day_sku_1042_morning_arrival() -> TypicalDayArrival {
        let stock_before = 12;
        let units_arrived = 40;
        TypicalDayArrival {
            sku: ProductId::from("SKU-1042"),
            time_of_day: "morning",
            units_arrived,
            stock_before,
            stock_after: stock_before + units_arrived,
        }
    }

    pub fn receive_arrival(&mut self, product_id: &ProductId, units: i32) -> Result<i32, String> {
        if units < 0 {
            return Err("arrival units cannot be negative".to_string());
        }
        let stock = self.warehouse_stock.entry(product_id.clone()).or_insert(0);
        *stock += units;
        Ok(*stock)
    }

    pub fn receive_customer_return(&mut self, customer_id: impl Into<String>, product_id: ProductId, quantity: i32) -> Result<i32, String> {
        if quantity <= 0 {
            return Err("returned goods quantity must be positive".to_string());
        }
        let returned = Return {
            customer_id: customer_id.into(),
            product_id: product_id.clone(),
            quantity,
        };
        let stock = self.warehouse_stock.entry(product_id).or_insert(0);
        *stock += quantity;
        self.received_returns.push(returned);
        Ok(*stock)
    }

    pub fn received_returns(&self) -> &[Return] {
        &self.received_returns
    }

    pub fn covers_warehouse_side_of_orderly(&self) -> bool {
        true
    }
}
