// src/catalog_category.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogCategory {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductCatalogMembership {
    pub product_id: String,
    pub category: CatalogCategory,
}

impl ProductCatalogMembership {
    pub fn new(product_id: impl Into<String>, category: CatalogCategory) -> Self {
        Self {
            product_id: product_id.into(),
            category,
        }
    }

    pub fn category(&self) -> &CatalogCategory {
        &self.category
    }
}
