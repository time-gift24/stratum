//! Postgres backend for execution-layer storage.
//!
//! One Postgres schema carries all execution facts: the durable agent-loop
//! event journal (`durable_events`), per-agent runtime state (`agent_state`),
//! and the committed message history (`agent_messages`). [`PostgresBackend`]
//! owns the shared connection pool and applies migrations on connect; the
//! [`PostgresAgentStore`] and [`PostgresDurableEventSink`] implementations are
//! constructed from it.

pub mod error;
mod events;
mod store;
mod tx;

pub use error::{PostgresError, PostgresEventSinkError};
pub use events::{PostgresDurableEventSink, read_events};
pub use store::PostgresAgentStore;

use sqlx::PgPool;
use stratum_core::{AgentId, SessionId, TurnId};

/// Shared Postgres backend handle: one connection pool over a migrated schema.
#[derive(Clone)]
pub struct PostgresBackend {
    pool: PgPool,
}

impl PostgresBackend {
    /// Connects to Postgres and applies all pending schema migrations.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::Connect`] when the database is unreachable and
    /// [`PostgresError::Migrate`] when the schema cannot be brought up to date.
    pub async fn connect(database_url: &str) -> Result<Self, PostgresError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(PostgresError::Connect)?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(PostgresError::Migrate)?;
        Ok(Self { pool })
    }

    /// Returns the shared connection pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Creates an agent store bound to one agent identity.
    #[must_use]
    pub fn agent_store(&self, agent_id: AgentId) -> PostgresAgentStore {
        PostgresAgentStore::new(self.pool.clone(), agent_id)
    }

    /// Opens a durable event sink bound to one run.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted sequence frontier of the run cannot
    /// be read.
    pub async fn event_sink(
        &self,
        session_id: SessionId,
        agent_id: AgentId,
        turn_id: TurnId,
    ) -> Result<PostgresDurableEventSink, PostgresEventSinkError> {
        PostgresDurableEventSink::open(self.pool.clone(), session_id, agent_id, turn_id).await
    }
}
