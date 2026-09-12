// src/picking.rs

/// Picking: collecting Products from shelves for a Shipment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picking {
    pub product_id: u32,
    pub shipment_id: u32,
    pub quantity: u32,
    pub source: &'static str,
}

impl Picking {
    pub fn collect_products_from_shelves_for_shipment(
        product_id: u32,
        shipment_id: u32,
        quantity: u32,
    ) -> Result<Self, &'static str> {
        if quantity == 0 {
            return Err("Picking requires at least one Product");
        }

        Ok(Self {
            product_id,
            shipment_id,
            quantity,
            source: "shelves",
        })
    }

    pub fn collects_product_for_shipment(&self, product_id: u32, shipment_id: u32) -> bool {
        self.product_id == product_id && self.shipment_id == shipment_id && self.source == "shelves"
    }
}
