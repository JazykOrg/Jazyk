// src/sku.rs

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sku(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SellableProductVariant {
    pub product_id: u32,
    pub variant_name: String,
    pub warehouse_label: Sku,
}

impl Sku {
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SellableProductVariant {
    // req:req:glossary-1 hash:3e21216a
    pub fn with_warehouse_label(product_id: u32, variant_name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            product_id,
            variant_name: variant_name.into(),
            warehouse_label: Sku::new(label),
        }
    }
}
