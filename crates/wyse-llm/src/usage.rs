//! Token cost helpers.

/// Per-token prices supplied by callers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenPrices {
    /// Input token price per million tokens.
    pub input_per_million: f64,
    /// Output token price per million tokens.
    pub output_per_million: f64,
}

/// Estimated cost for one usage record.
#[derive(Debug, Clone, PartialEq)]
pub struct CostEstimate {
    /// Currency code for the estimate.
    pub currency: String,
    /// Total estimated cost.
    pub total: f64,
}
