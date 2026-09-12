#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypicalDaySku1042Arrival {
    pub sku: &'static str,
    pub time_of_day: &'static str,
    pub units_arriving: u32,
    pub stock_before: u32,
    pub stock_after: u32,
}

// req:req:inventory-4 hash:bdea2213
pub fn morning_arrival_raises_stock(stock_before: u32) -> TypicalDaySku1042Arrival {
    let units_arriving = 40;
    TypicalDaySku1042Arrival {
        sku: "SKU-1042",
        time_of_day: "morning",
        units_arriving,
        stock_before,
        stock_after: stock_before + units_arriving,
    }
}
