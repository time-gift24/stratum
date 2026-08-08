//! Stratum API process entry point.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let telemetry = stratum_api::init_telemetry()?;
    stratum_api::run_from_path("config.toml").await?;
    // Graceful shutdown ends here: flush and end the OTLP tracer provider.
    telemetry.shutdown();
    Ok(())
}
