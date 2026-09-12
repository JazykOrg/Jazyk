#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductStock {
    pub product_id: String,
    pub stock: i32,
    pub reorder_point: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestockTask {
    pub product_id: String,
    pub current_stock: i32,
    pub reorder_point: i32,
}

#[derive(Debug, Default)]
pub struct Inventory {
    restock_tasks: Vec<RestockTask>,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate_reorder_point(&mut self, product: &ProductStock) -> Option<RestockTask> {
        if product.stock <= product.reorder_point {
            let task = RestockTask {
                product_id: product.product_id.clone(),
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
}
