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
    pub name: String,
    pub stock: u32,
    pub reorder_point: u32,
}

impl Product {
    pub fn new(id: impl Into<ProductId>, name: impl Into<String>, stock: u32, reorder_point: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            stock,
            reorder_point,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestockTask {
    pub product_id: ProductId,
    pub current_stock: u32,
    pub reorder_point: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShipmentPick {
    pub product_id: ProductId,
    pub picked_quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderItem {
    pub product_id: ProductId,
    pub quantity: u32,
    pub stock_reserved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: String,
    pub paid: bool,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shipment {
    pub order_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedReturn {
    pub product_id: ProductId,
    pub quantity: u32,
    pub received: bool,
    pub inspected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypicalDayArrival {
    pub sku: ProductId,
    pub time_of_day: &'static str,
    pub units_arrived: u32,
    pub stock_before: u32,
    pub stock_after: u32,
}

#[derive(Debug, Default)]
pub struct Inventory {
    stock_by_product: BTreeMap<ProductId, u32>,
    reorder_points: BTreeMap<ProductId, u32>,
    restock_tasks: Vec<RestockTask>,
}

impl Inventory {
    // req:req:inventory-1 hash:e8972aee
    pub fn track_product(&mut self, product: Product) {
        self.reorder_points.insert(product.id.clone(), product.reorder_point);
        self.stock_by_product.insert(product.id, product.stock);
    }

    pub fn stock_for(&self, product_id: &ProductId) -> Option<u32> {
        self.stock_by_product.get(product_id).copied()
    }

    // req:req:inventory-2 hash:69ab75c3
    pub fn pick_for_shipment(&mut self, pick: ShipmentPick) -> Result<u32, String> {
        let stock = self
            .stock_by_product
            .get_mut(&pick.product_id)
            .ok_or_else(|| format!("unknown product {}", pick.product_id.0))?;
        if *stock < pick.picked_quantity {
            return Err("picked quantity would take Stock below zero".to_string());
        }
        *stock -= pick.picked_quantity;
        let remaining = *stock;
        self.create_restock_task_if_reorder_point_reached(&pick.product_id);
        Ok(remaining)
    }

    // req:req:inventory-3 hash:900f93cb
    pub fn create_restock_task_if_reorder_point_reached(&mut self, product_id: &ProductId) -> Option<RestockTask> {
        let current_stock = *self.stock_by_product.get(product_id)?;
        let reorder_point = *self.reorder_points.get(product_id)?;
        if current_stock <= reorder_point {
            let task = RestockTask {
                product_id: product_id.clone(),
                current_stock,
                reorder_point,
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

    // req:req:inventory-4 hash:bdea2213
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

    pub fn receive_arrival(&mut self, product_id: ProductId, units: u32) -> u32 {
        let stock = self.stock_by_product.entry(product_id).or_insert(0);
        *stock += units;
        *stock
    }

    // req:req:inventory-5 hash:4a774f90
    pub fn decrease_without_negative_stock(&mut self, product_id: &ProductId, quantity: u32) -> Result<u32, String> {
        let stock = self
            .stock_by_product
            .get_mut(product_id)
            .ok_or_else(|| format!("unknown product {}", product_id.0))?;
        if *stock < quantity {
            return Err("Stock count shall never go below zero".to_string());
        }
        *stock -= quantity;
        Ok(*stock)
    }

    // req:req:orders-3 hash:ebd76a10
    pub fn reserve_stock_for_order(&mut self, order: &mut Order) -> Result<(), String> {
        for item in &order.items {
            let stock = self.stock_by_product.get(&item.product_id).copied().unwrap_or(0);
            if stock < item.quantity {
                return Err(format!("not enough Stock for {}", item.product_id.0));
            }
        }
        for item in &mut order.items {
            let stock = self.stock_by_product.get_mut(&item.product_id).expect("checked above");
            *stock -= item.quantity;
            item.stock_reserved = true;
        }
        Ok(())
    }

    // req:req:returns-4 hash:264fa475
    pub fn restore_stock_from_received_inspected_return(&mut self, returned: &InspectedReturn) -> Result<u32, String> {
        if !(returned.received && returned.inspected) {
            return Err("Return must be received and inspected before Stock is restored".to_string());
        }
        Ok(self.receive_arrival(returned.product_id.clone(), returned.quantity))
    }

    // req:req:shipping-2 hash:58de09dc
    pub fn create_shipment_when_paid_and_in_stock(&self, order: &Order) -> Option<Shipment> {
        if !order.paid {
            return None;
        }
        let every_product_in_stock = order.items.iter().all(|item| {
            self.stock_by_product
                .get(&item.product_id)
                .copied()
                .unwrap_or(0)
                >= item.quantity
        });
        every_product_in_stock.then(|| Shipment { order_id: order.id.clone() })
    }
}

#[derive(Debug, Default)]
pub struct Catalog {
    products: BTreeMap<ProductId, Product>,
}

impl Catalog {
    pub fn add_product(&mut self, product: Product) {
        self.products.insert(product.id.clone(), product);
    }

    // req:req:catalog-4 hash:72d3d04b
    pub fn visible_products_in_stock<'a>(&'a self, inventory: &Inventory) -> Vec<&'a Product> {
        self.products
            .values()
            .filter(|product| inventory.stock_for(&product.id).unwrap_or(0) > 0)
            .collect()
    }

    // req:req:catalog-5 hash:84eddf06
    pub fn hides_product_when_stock_reaches_zero(&self, product_id: &ProductId, inventory: &Inventory) -> bool {
        inventory.stock_for(product_id).unwrap_or(0) == 0
            && !self
                .visible_products_in_stock(inventory)
                .iter()
                .any(|product| &product.id == product_id)
    }
}

pub struct ReorderPoint;

impl ReorderPoint {
    // req:req:glossary-3 hash:0d257092
    pub const DEFINITION: &'static str = "Reorder point: the Stock level that triggers restocking.";

    pub fn triggers_restocking(stock_level: u32, reorder_point: u32) -> bool {
        stock_level <= reorder_point
    }
}
