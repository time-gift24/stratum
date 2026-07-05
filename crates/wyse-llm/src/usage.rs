//! Token cost helpers.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenPrices {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostEstimate {
    pub currency: String,
    pub total: f64,
}
