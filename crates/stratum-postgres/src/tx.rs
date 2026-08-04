//! Crate-private transaction helpers.

use sqlx::{PgConnection, PgPool};
use stratum_store::StoreError;

/// Runs `operation` inside one transaction, committing on success and rolling
/// back (via drop) when it returns an error.
pub(crate) async fn run_in_transaction<T>(
    pool: &PgPool,
    operation: impl AsyncFnOnce(&mut PgConnection) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let mut transaction = pool.begin().await.map_err(StoreError::backend)?;
    let result = operation(&mut transaction).await?;
    transaction.commit().await.map_err(StoreError::backend)?;
    Ok(result)
}
