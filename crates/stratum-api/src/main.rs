//! Stratum API process entry point.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let telemetry = stratum_api::init_telemetry()?;
    let result = stratum_api::run_from_path("config.toml").await;
    // Graceful shutdown ends here: flush and end the OTLP tracer provider.
    telemetry.shutdown();
    result.map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
}
