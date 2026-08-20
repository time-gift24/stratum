use std::collections::HashSet;

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use stratum_core::{AgentName, AgentVersionTag, ModelConfig, ModelId, ToolName};
use uuid::Uuid;

use crate::{
    AgentDefinition, AgentDefinitionInput, DeletionBlocker, ManagedModel, ModelCatalogSnapshot,
    ProviderKind, ProviderSummary, ResourceVersion, RuntimeProvider, StudioError, Versioned,
};

/// Concrete access to the isolated Studio management database.
#[derive(Clone, Debug)]
pub struct StudioStore {
    pool: PgPool,
}

impl StudioStore {
    /// Connects to the Studio database and applies this crate's migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when the database cannot be connected or its
    /// migration history cannot be applied.
    #[tracing::instrument(skip_all, fields(operation = "studio.connect"))]
    pub async fn connect(database_url: &str) -> Result<Self, StudioError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Checks whether the Studio database accepts queries.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when the database is unavailable.
    #[tracing::instrument(skip_all, fields(operation = "studio.ping"))]
    pub async fn ping(&self) -> Result<(), StudioError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Lists sanitized Provider summaries in stable display order.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when the catalog cannot be read.
    #[tracing::instrument(skip_all, fields(operation = "studio.providers.list"))]
    pub async fn list_providers(&self) -> Result<Vec<Versioned<ProviderSummary>>, StudioError> {
        self.require_catalog().await?;
        let rows = sqlx::query(
            "SELECT p.kind, p.revision, p.updated_at, \
             EXISTS(SELECT 1 FROM studio_provider_credentials c WHERE c.provider_kind = p.kind) \
                 AS credential_configured, \
             (SELECT COUNT(*) FROM studio_models m WHERE m.provider_kind = p.kind) AS models_count \
             FROM studio_providers p \
             ORDER BY CASE p.kind WHEN 'openai' THEN 0 WHEN 'deepseek' THEN 1 END",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(provider_summary_from_row).collect()
    }

    /// Reads one sanitized Provider summary.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::NotFound`] when `kind` is absent.
    #[tracing::instrument(
        skip_all,
        fields(operation = "studio.provider.get", provider = %kind)
    )]
    pub async fn provider(
        &self,
        kind: ProviderKind,
    ) -> Result<Versioned<ProviderSummary>, StudioError> {
        self.require_catalog().await?;
        let row = sqlx::query(
            "SELECT p.kind, p.revision, p.updated_at, \
             EXISTS(SELECT 1 FROM studio_provider_credentials c WHERE c.provider_kind = p.kind) \
                 AS credential_configured, \
             (SELECT COUNT(*) FROM studio_models m WHERE m.provider_kind = p.kind) AS models_count \
             FROM studio_providers p WHERE p.kind = $1",
        )
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StudioError::NotFound)?;
        provider_summary_from_row(row)
    }

    /// Creates a Provider and records its credential without exposing it.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::AlreadyExists`] when the Provider is present, or
    /// [`StudioError::InvalidInput`] for a blank credential.
    #[tracing::instrument(
        skip_all,
        fields(operation = "studio.provider.create", provider = %kind)
    )]
    pub async fn create_provider(
        &self,
        kind: ProviderKind,
        api_key: SecretString,
    ) -> Result<Versioned<ProviderSummary>, StudioError> {
        validate_api_key(&api_key)?;
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        let inserted = sqlx::query(
            "INSERT INTO studio_providers (kind, revision) VALUES ($1, $2) \
             ON CONFLICT (kind) DO NOTHING",
        )
        .bind(kind.as_str())
        .bind(Uuid::now_v7())
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            return Err(StudioError::AlreadyExists);
        }
        sqlx::query(
            "INSERT INTO studio_provider_credentials (provider_kind, secret) VALUES ($1, $2)",
        )
        .bind(kind.as_str())
        .bind(api_key.expose_secret())
        .execute(&mut *transaction)
        .await?;
        bump_catalog(&mut transaction).await?;
        let created = provider_in_transaction(&mut transaction, kind).await?;
        transaction.commit().await?;
        Ok(created)
    }

    /// Replaces an existing Provider credential when its version still matches.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::PreconditionFailed`] for a stale version and
    /// [`StudioError::NotFound`] when the Provider is absent.
    #[tracing::instrument(
        skip_all,
        fields(operation = "studio.provider.replace_credential", provider = %kind)
    )]
    pub async fn replace_provider_credential(
        &self,
        kind: ProviderKind,
        api_key: SecretString,
        expected: ResourceVersion,
    ) -> Result<Versioned<ProviderSummary>, StudioError> {
        validate_api_key(&api_key)?;
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        require_provider_version(&mut transaction, kind, expected).await?;
        sqlx::query(
            "INSERT INTO studio_provider_credentials (provider_kind, secret, updated_at) \
             VALUES ($1, $2, NOW()) \
             ON CONFLICT (provider_kind) DO UPDATE SET secret = EXCLUDED.secret, updated_at = NOW()",
        )
        .bind(kind.as_str())
        .bind(api_key.expose_secret())
        .execute(&mut *transaction)
        .await?;
        update_provider_version(&mut transaction, kind).await?;
        bump_catalog(&mut transaction).await?;
        let updated = provider_in_transaction(&mut transaction, kind).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Deletes a Provider and its managed models when no Agent definition
    /// selects any of those models.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::DeletionBlocked`] when Agent definitions must be
    /// changed or removed first.
    #[tracing::instrument(
        skip_all,
        fields(operation = "studio.provider.delete", provider = %kind)
    )]
    pub async fn delete_provider(
        &self,
        kind: ProviderKind,
        expected: ResourceVersion,
    ) -> Result<(), StudioError> {
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        require_provider_version(&mut transaction, kind, expected).await?;

        let agent_rows = sqlx::query(
            "SELECT definition.agent_name \
             FROM studio_agent_definitions definition \
             JOIN studio_models model \
               ON definition.model_id = model.provider_kind || ':' || model.name \
             WHERE model.provider_kind = $1 \
             ORDER BY definition.agent_name",
        )
        .bind(kind.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let blockers = agent_rows
            .into_iter()
            .map(|row| DeletionBlocker::agent_definition(row.get("agent_name")))
            .collect::<Vec<_>>();
        if !blockers.is_empty() {
            return Err(StudioError::DeletionBlocked { blockers });
        }

        sqlx::query("DELETE FROM studio_models WHERE provider_kind = $1")
            .bind(kind.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM studio_provider_credentials WHERE provider_kind = $1")
            .bind(kind.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM studio_providers WHERE kind = $1")
            .bind(kind.as_str())
            .execute(&mut *transaction)
            .await?;
        bump_catalog(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Lists all managed models in stable Provider/name order.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when the catalog cannot be read.
    #[tracing::instrument(skip_all, fields(operation = "studio.models.list"))]
    pub async fn list_models(&self) -> Result<Vec<Versioned<ManagedModel>>, StudioError> {
        self.require_catalog().await?;
        let rows = sqlx::query(
            "SELECT provider_kind, name, revision, updated_at FROM studio_models \
             ORDER BY provider_kind, name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(model_from_row).collect()
    }

    /// Reads one managed model.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::NotFound`] when the model is absent.
    #[tracing::instrument(
        skip_all,
        fields(
            operation = "studio.model.get",
            provider = %kind,
            model_name = %name
        )
    )]
    pub async fn model(
        &self,
        kind: ProviderKind,
        name: &str,
    ) -> Result<Versioned<ManagedModel>, StudioError> {
        self.require_catalog().await?;
        let row = sqlx::query(
            "SELECT provider_kind, name, revision, updated_at FROM studio_models \
             WHERE provider_kind = $1 AND name = $2",
        )
        .bind(kind.as_str())
        .bind(name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StudioError::NotFound)?;
        model_from_row(row)
    }

    /// Creates a provider-local model.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::ModelNotConfigured`] when its Provider is absent.
    #[tracing::instrument(
        skip_all,
        fields(
            operation = "studio.model.create",
            provider = %kind,
            model_name = %name.as_str()
        )
    )]
    pub async fn create_model(
        &self,
        kind: ProviderKind,
        name: String,
    ) -> Result<Versioned<ManagedModel>, StudioError> {
        validate_model_name(kind, &name)?;
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        ensure_provider_exists(&mut transaction, kind).await?;
        let inserted = sqlx::query(
            "INSERT INTO studio_models (provider_kind, name, revision) VALUES ($1, $2, $3) \
             ON CONFLICT (provider_kind, name) DO NOTHING",
        )
        .bind(kind.as_str())
        .bind(&name)
        .bind(Uuid::now_v7())
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            return Err(StudioError::AlreadyExists);
        }
        update_provider_version(&mut transaction, kind).await?;
        bump_catalog(&mut transaction).await?;
        let created = model_in_transaction(&mut transaction, kind, &name).await?;
        transaction.commit().await?;
        Ok(created)
    }

    /// Deletes a model when no Agent definition selects it.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::DeletionBlocked`] when Agent definitions still
    /// select the model.
    #[tracing::instrument(
        skip_all,
        fields(
            operation = "studio.model.delete",
            provider = %kind,
            model_name = %name
        )
    )]
    pub async fn delete_model(
        &self,
        kind: ProviderKind,
        name: &str,
        expected: ResourceVersion,
    ) -> Result<(), StudioError> {
        let model_id = ModelId::new(kind.as_str(), name)
            .map_err(|_| StudioError::InvalidInput { field: "name" })?;
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        require_model_version(&mut transaction, kind, name, expected).await?;
        let rows = sqlx::query(
            "SELECT agent_name FROM studio_agent_definitions WHERE model_id = $1 ORDER BY agent_name",
        )
        .bind(model_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let blockers = rows
            .into_iter()
            .map(|row| DeletionBlocker::agent_definition(row.get("agent_name")))
            .collect::<Vec<_>>();
        if !blockers.is_empty() {
            return Err(StudioError::DeletionBlocked { blockers });
        }
        sqlx::query("DELETE FROM studio_models WHERE provider_kind = $1 AND name = $2")
            .bind(kind.as_str())
            .bind(name)
            .execute(&mut *transaction)
            .await?;
        update_provider_version(&mut transaction, kind).await?;
        bump_catalog(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Lists mutable Agent authoring definitions in stable name order.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when a stored value cannot be read or validated.
    #[tracing::instrument(skip_all, fields(operation = "studio.agent_definitions.list"))]
    pub async fn list_agent_definitions(
        &self,
    ) -> Result<Vec<Versioned<AgentDefinition>>, StudioError> {
        self.require_catalog().await?;
        let rows = sqlx::query(
            "SELECT agent_name, version, model_id, model_parameters, tools, prompt, revision, updated_at \
             FROM studio_agent_definitions ORDER BY agent_name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(agent_definition_from_row).collect()
    }

    /// Reads one deterministic Agent-definition page without loading prompts
    /// and parameters outside that page.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when pagination exceeds the SQL boundary or a
    /// stored definition is unavailable or corrupt.
    #[tracing::instrument(skip_all, fields(operation = "studio.agent_definitions.page"))]
    pub async fn page_agent_definitions(
        &self,
        search: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Versioned<AgentDefinition>>, usize), StudioError> {
        let offset = i64::try_from(offset).map_err(|_| StudioError::InvalidInput {
            field: "pagination",
        })?;
        let limit = i64::try_from(limit).map_err(|_| StudioError::InvalidInput {
            field: "pagination",
        })?;
        let mut transaction = self.pool.begin().await?;
        lock_catalog_shared(&mut transaction).await?;
        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM studio_agent_definitions \
             WHERE $1::TEXT IS NULL \
                OR POSITION(LOWER($1::TEXT) IN LOWER(agent_name)) > 0",
        )
        .bind(search)
        .fetch_one(&mut *transaction)
        .await?;
        let rows = sqlx::query(
            "SELECT agent_name, version, model_id, model_parameters, tools, prompt, \
                    revision, updated_at \
             FROM studio_agent_definitions \
             WHERE $1::TEXT IS NULL \
                OR POSITION(LOWER($1::TEXT) IN LOWER(agent_name)) > 0 \
             ORDER BY updated_at DESC, agent_name ASC \
             LIMIT $2 OFFSET $3",
        )
        .bind(search)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut *transaction)
        .await?;
        let definitions = rows
            .into_iter()
            .map(agent_definition_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let total = usize::try_from(total).map_err(|_| StudioError::CatalogCorrupt {
            field: "agent_definition_count",
        })?;
        transaction.commit().await?;
        Ok((definitions, total))
    }

    /// Reads one Agent authoring definition.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::NotFound`] when `agent_name` is absent.
    #[tracing::instrument(
        skip_all,
        fields(operation = "studio.agent_definition.get", agent_name = %agent_name)
    )]
    pub async fn agent_definition(
        &self,
        agent_name: &AgentName,
    ) -> Result<Versioned<AgentDefinition>, StudioError> {
        self.require_catalog().await?;
        let row = sqlx::query(
            "SELECT agent_name, version, model_id, model_parameters, tools, prompt, revision, updated_at \
             FROM studio_agent_definitions WHERE agent_name = $1",
        )
        .bind(agent_name.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StudioError::NotFound)?;
        agent_definition_from_row(row)
    }

    /// Creates a mutable Agent authoring definition for future runtimes.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::AlreadyExists`] when the name is already managed,
    /// or [`StudioError::ModelNotConfigured`] for an absent model.
    #[tracing::instrument(
        skip_all,
        fields(
            operation = "studio.agent_definition.create",
            agent_name = %definition.agent_name.as_str()
        )
    )]
    pub async fn create_agent_definition(
        &self,
        definition: AgentDefinitionInput,
    ) -> Result<Versioned<AgentDefinition>, StudioError> {
        validate_definition(&definition)?;
        let agent_name = definition.agent_name.clone();
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        ensure_model_configured(&mut transaction, &definition.model.model).await?;
        let inserted = sqlx::query(
            "INSERT INTO studio_agent_definitions \
             (agent_name, version, model_id, model_parameters, tools, prompt, revision) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (agent_name) DO NOTHING",
        )
        .bind(agent_name.as_str())
        .bind(definition.agent_version.as_str())
        .bind(definition.model.model.as_str())
        .bind(Value::Object(definition.model.parameters))
        .bind(
            serde_json::to_value(&definition.tools)
                .map_err(|_| StudioError::InvalidInput { field: "tools" })?,
        )
        .bind(definition.prompt)
        .bind(Uuid::now_v7())
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            return Err(StudioError::AlreadyExists);
        }
        bump_catalog(&mut transaction).await?;
        let created = agent_definition_in_transaction(&mut transaction, &agent_name).await?;
        transaction.commit().await?;
        Ok(created)
    }

    /// Replaces an Agent definition only with a new author version tag.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::AgentVersionUnchanged`] when the behavior would
    /// change under the same immutable version identity.
    #[tracing::instrument(
        skip_all,
        fields(
            operation = "studio.agent_definition.replace",
            agent_name = %definition.agent_name.as_str()
        )
    )]
    pub async fn replace_agent_definition(
        &self,
        definition: AgentDefinitionInput,
        expected: ResourceVersion,
    ) -> Result<Versioned<AgentDefinition>, StudioError> {
        validate_definition(&definition)?;
        let agent_name = definition.agent_name.clone();
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        let row = sqlx::query(
            "SELECT version, revision FROM studio_agent_definitions WHERE agent_name = $1 FOR UPDATE",
        )
        .bind(agent_name.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StudioError::NotFound)?;
        let actual = ResourceVersion::new(row.get("revision"));
        if actual != expected {
            return Err(StudioError::PreconditionFailed);
        }
        let current_version: String = row.get("version");
        if current_version == definition.agent_version.as_str() {
            return Err(StudioError::AgentVersionUnchanged);
        }
        ensure_model_configured(&mut transaction, &definition.model.model).await?;
        sqlx::query(
            "UPDATE studio_agent_definitions SET version = $2, model_id = $3, model_parameters = $4, \
             tools = $5, prompt = $6, revision = $7, updated_at = NOW() WHERE agent_name = $1",
        )
        .bind(agent_name.as_str())
        .bind(definition.agent_version.as_str())
        .bind(definition.model.model.as_str())
        .bind(Value::Object(definition.model.parameters))
        .bind(serde_json::to_value(&definition.tools).map_err(|_| StudioError::InvalidInput {
            field: "tools",
        })?)
        .bind(definition.prompt)
        .bind(Uuid::now_v7())
        .execute(&mut *transaction)
        .await?;
        bump_catalog(&mut transaction).await?;
        let updated = agent_definition_in_transaction(&mut transaction, &agent_name).await?;
        transaction.commit().await?;
        Ok(updated)
    }

    /// Deletes a mutable Agent authoring definition.
    ///
    /// Already-created AgentRuntime records keep their independent, immutable
    /// definition snapshots in the execution ledger.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError::PreconditionFailed`] for a stale version.
    #[tracing::instrument(
        skip_all,
        fields(operation = "studio.agent_definition.delete", agent_name = %agent_name)
    )]
    pub async fn delete_agent_definition(
        &self,
        agent_name: &AgentName,
        expected: ResourceVersion,
    ) -> Result<(), StudioError> {
        let mut transaction = self.pool.begin().await?;
        lock_catalog(&mut transaction).await?;
        let row = sqlx::query(
            "SELECT revision FROM studio_agent_definitions WHERE agent_name = $1 FOR UPDATE",
        )
        .bind(agent_name.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StudioError::NotFound)?;
        if ResourceVersion::new(row.get("revision")) != expected {
            return Err(StudioError::PreconditionFailed);
        }
        sqlx::query("DELETE FROM studio_agent_definitions WHERE agent_name = $1")
            .bind(agent_name.as_str())
            .execute(&mut *transaction)
            .await?;
        bump_catalog(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Reads credentials and models only for trusted runtime assembly.
    ///
    /// This method is intentionally not suitable for HTTP serialization. Its
    /// values are secret-bearing and must remain inside the API-to-provider
    /// construction path.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when catalog values cannot be safely loaded.
    #[tracing::instrument(skip_all, fields(operation = "studio.runtime_providers.load"))]
    pub async fn runtime_providers(&self) -> Result<Vec<RuntimeProvider>, StudioError> {
        let mut transaction = self.pool.begin().await?;
        lock_catalog_shared(&mut transaction).await?;
        let providers = runtime_providers_in_transaction(&mut transaction).await?;
        transaction.commit().await?;
        Ok(providers)
    }

    /// Reads Model representations and their adapter inputs while holding one
    /// shared catalog lock.
    ///
    /// Management handlers use this boundary so a concurrent Provider/Model
    /// write cannot combine a Model row from one revision with credentials or
    /// adapter membership from another revision.
    ///
    /// # Errors
    ///
    /// Returns [`StudioError`] when the catalog is unavailable or corrupt.
    #[tracing::instrument(skip_all, fields(operation = "studio.model_catalog.load"))]
    pub async fn model_catalog_snapshot(&self) -> Result<ModelCatalogSnapshot, StudioError> {
        let mut transaction = self.pool.begin().await?;
        lock_catalog_shared(&mut transaction).await?;
        let rows = sqlx::query(
            "SELECT provider_kind, name, revision, updated_at FROM studio_models \
             ORDER BY provider_kind, name",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let models = rows
            .into_iter()
            .map(model_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let runtime_providers = runtime_providers_in_transaction(&mut transaction).await?;
        transaction.commit().await?;
        Ok(ModelCatalogSnapshot {
            models,
            runtime_providers,
        })
    }

    async fn require_catalog(&self) -> Result<(), StudioError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM studio_catalog WHERE singleton = TRUE)",
        )
        .fetch_one(&self.pool)
        .await?;
        if exists {
            Ok(())
        } else {
            Err(StudioError::NotInitialized)
        }
    }
}

async fn runtime_providers_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Vec<RuntimeProvider>, StudioError> {
    let rows = sqlx::query(
        "SELECT p.kind, c.secret FROM studio_providers p \
             LEFT JOIN studio_provider_credentials c ON c.provider_kind = p.kind \
             ORDER BY p.kind",
    )
    .fetch_all(&mut **transaction)
    .await?;
    let mut providers = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: ProviderKind =
            row.get::<String, _>("kind")
                .parse()
                .map_err(|_| StudioError::CatalogCorrupt {
                    field: "provider_kind",
                })?;
        let model_rows =
            sqlx::query("SELECT name FROM studio_models WHERE provider_kind = $1 ORDER BY name")
                .bind(kind.as_str())
                .fetch_all(&mut **transaction)
                .await?;
        let secret = row
            .get::<Option<String>, _>("secret")
            .ok_or(StudioError::CatalogCorrupt {
                field: "provider_credential",
            })?;
        if secret.trim().is_empty() {
            return Err(StudioError::CatalogCorrupt {
                field: "provider_credential",
            });
        }
        providers.push(RuntimeProvider {
            kind,
            api_key: SecretString::from(secret),
            models: model_rows.into_iter().map(|row| row.get("name")).collect(),
        });
    }
    Ok(providers)
}

async fn lock_catalog_shared(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StudioError> {
    let row = sqlx::query("SELECT revision FROM studio_catalog WHERE singleton = TRUE FOR SHARE")
        .fetch_optional(&mut **transaction)
        .await?;
    if row.is_some() {
        Ok(())
    } else {
        Err(StudioError::NotInitialized)
    }
}

async fn lock_catalog(transaction: &mut Transaction<'_, Postgres>) -> Result<(), StudioError> {
    let row = sqlx::query("SELECT revision FROM studio_catalog WHERE singleton = TRUE FOR UPDATE")
        .fetch_optional(&mut **transaction)
        .await?;
    if row.is_some() {
        Ok(())
    } else {
        Err(StudioError::NotInitialized)
    }
}

async fn bump_catalog(transaction: &mut Transaction<'_, Postgres>) -> Result<(), StudioError> {
    sqlx::query("UPDATE studio_catalog SET revision = $1 WHERE singleton = TRUE")
        .bind(Uuid::now_v7())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn ensure_provider_exists(
    transaction: &mut Transaction<'_, Postgres>,
    kind: ProviderKind,
) -> Result<(), StudioError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM studio_providers WHERE kind = $1)",
    )
    .bind(kind.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(StudioError::NotFound)
    }
}

async fn ensure_model_configured(
    transaction: &mut Transaction<'_, Postgres>,
    model: &ModelId,
) -> Result<(), StudioError> {
    let kind = ProviderKind::from_model_id(model)
        .map_err(|_| StudioError::InvalidInput { field: "model" })?;
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM studio_models WHERE provider_kind = $1 AND name = $2)",
    )
    .bind(kind.as_str())
    .bind(model.model_name())
    .fetch_one(&mut **transaction)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(StudioError::ModelNotConfigured)
    }
}

async fn require_provider_version(
    transaction: &mut Transaction<'_, Postgres>,
    kind: ProviderKind,
    expected: ResourceVersion,
) -> Result<(), StudioError> {
    let row = sqlx::query("SELECT revision FROM studio_providers WHERE kind = $1 FOR UPDATE")
        .bind(kind.as_str())
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(StudioError::NotFound)?;
    if ResourceVersion::new(row.get("revision")) == expected {
        Ok(())
    } else {
        Err(StudioError::PreconditionFailed)
    }
}

async fn require_model_version(
    transaction: &mut Transaction<'_, Postgres>,
    kind: ProviderKind,
    name: &str,
    expected: ResourceVersion,
) -> Result<(), StudioError> {
    let row = sqlx::query(
        "SELECT revision FROM studio_models WHERE provider_kind = $1 AND name = $2 FOR UPDATE",
    )
    .bind(kind.as_str())
    .bind(name)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StudioError::NotFound)?;
    if ResourceVersion::new(row.get("revision")) == expected {
        Ok(())
    } else {
        Err(StudioError::PreconditionFailed)
    }
}

async fn update_provider_version(
    transaction: &mut Transaction<'_, Postgres>,
    kind: ProviderKind,
) -> Result<(), StudioError> {
    sqlx::query("UPDATE studio_providers SET revision = $2, updated_at = NOW() WHERE kind = $1")
        .bind(kind.as_str())
        .bind(Uuid::now_v7())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn provider_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    kind: ProviderKind,
) -> Result<Versioned<ProviderSummary>, StudioError> {
    let row = sqlx::query(
        "SELECT p.kind, p.revision, p.updated_at, \
         EXISTS(SELECT 1 FROM studio_provider_credentials c WHERE c.provider_kind = p.kind) \
             AS credential_configured, \
         (SELECT COUNT(*) FROM studio_models m WHERE m.provider_kind = p.kind) AS models_count \
         FROM studio_providers p WHERE p.kind = $1",
    )
    .bind(kind.as_str())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StudioError::NotFound)?;
    provider_summary_from_row(row)
}

async fn model_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    kind: ProviderKind,
    name: &str,
) -> Result<Versioned<ManagedModel>, StudioError> {
    let row = sqlx::query(
        "SELECT provider_kind, name, revision, updated_at FROM studio_models \
         WHERE provider_kind = $1 AND name = $2",
    )
    .bind(kind.as_str())
    .bind(name)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StudioError::NotFound)?;
    model_from_row(row)
}

async fn agent_definition_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    agent_name: &AgentName,
) -> Result<Versioned<AgentDefinition>, StudioError> {
    let row = sqlx::query(
        "SELECT agent_name, version, model_id, model_parameters, tools, prompt, revision, updated_at \
         FROM studio_agent_definitions WHERE agent_name = $1",
    )
    .bind(agent_name.as_str())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StudioError::NotFound)?;
    agent_definition_from_row(row)
}

fn provider_summary_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<Versioned<ProviderSummary>, StudioError> {
    let kind = row
        .get::<String, _>("kind")
        .parse()
        .map_err(|_| StudioError::CatalogCorrupt {
            field: "provider_kind",
        })?;
    let models_count: i64 = row.get("models_count");
    let models_count = usize::try_from(models_count).map_err(|_| StudioError::CatalogCorrupt {
        field: "models_count",
    })?;
    let credential_configured: bool = row.get("credential_configured");
    if !credential_configured {
        return Err(StudioError::CatalogCorrupt {
            field: "provider_credential",
        });
    }
    Ok(Versioned {
        value: ProviderSummary {
            kind,
            credential_configured,
            models_count,
            updated_at: row.get::<DateTime<Utc>, _>("updated_at"),
        },
        version: ResourceVersion::new(row.get("revision")),
    })
}

fn model_from_row(row: sqlx::postgres::PgRow) -> Result<Versioned<ManagedModel>, StudioError> {
    let provider: ProviderKind =
        row.get::<String, _>("provider_kind")
            .parse()
            .map_err(|_| StudioError::CatalogCorrupt {
                field: "provider_kind",
            })?;
    let name: String = row.get("name");
    let model =
        ModelId::new(provider.as_str(), &name).map_err(|_| StudioError::CatalogCorrupt {
            field: "model_name",
        })?;
    Ok(Versioned {
        value: ManagedModel {
            model,
            provider,
            name,
            updated_at: row.get("updated_at"),
        },
        version: ResourceVersion::new(row.get("revision")),
    })
}

fn agent_definition_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<Versioned<AgentDefinition>, StudioError> {
    let agent_name = row
        .get::<String, _>("agent_name")
        .parse::<AgentName>()
        .map_err(|_| StudioError::CatalogCorrupt {
            field: "agent_name",
        })?;
    let agent_version = row
        .get::<String, _>("version")
        .parse::<AgentVersionTag>()
        .map_err(|_| StudioError::CatalogCorrupt { field: "version" })?;
    let model = row
        .get::<String, _>("model_id")
        .parse::<ModelId>()
        .map_err(|_| StudioError::CatalogCorrupt { field: "model_id" })?;
    let parameters = row
        .get::<Value, _>("model_parameters")
        .as_object()
        .cloned()
        .ok_or(StudioError::CatalogCorrupt {
            field: "model_parameters",
        })?;
    let tools = serde_json::from_value::<Vec<ToolName>>(row.get("tools"))
        .map_err(|_| StudioError::CatalogCorrupt { field: "tools" })?;
    Ok(Versioned {
        value: AgentDefinition {
            agent_name,
            agent_version,
            model: ModelConfig::new(model, parameters),
            tools,
            prompt: row.get("prompt"),
            updated_at: row.get("updated_at"),
        },
        version: ResourceVersion::new(row.get("revision")),
    })
}

fn validate_api_key(api_key: &SecretString) -> Result<(), StudioError> {
    if api_key.expose_secret().trim().is_empty() {
        Err(StudioError::InvalidInput { field: "api_key" })
    } else {
        Ok(())
    }
}

fn validate_model_name(kind: ProviderKind, name: &str) -> Result<(), StudioError> {
    ModelId::new(kind.as_str(), name)
        .map(|_| ())
        .map_err(|_| StudioError::InvalidInput { field: "name" })
}

fn validate_definition(definition: &AgentDefinitionInput) -> Result<(), StudioError> {
    if definition.prompt.trim().is_empty() {
        return Err(StudioError::InvalidInput { field: "prompt" });
    }
    let mut tools = HashSet::with_capacity(definition.tools.len());
    if definition
        .tools
        .iter()
        .any(|tool| !tools.insert(tool.as_str()))
    {
        return Err(StudioError::InvalidInput { field: "tools" });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::Map;
    use stratum_core::{AgentName, AgentVersionTag, ModelConfig, ModelId, ToolName};

    use super::validate_definition;
    use crate::{AgentDefinitionInput, StudioError};

    fn definition(version: &str) -> AgentDefinitionInput {
        AgentDefinitionInput {
            agent_name: AgentName::new("coding-agent").expect("valid agent name"),
            agent_version: AgentVersionTag::new(version).expect("valid version"),
            model: ModelConfig::new(
                ModelId::new("openai", "gpt-5").expect("valid model"),
                Map::new(),
            ),
            tools: vec![ToolName::new("filesystem")],
            prompt: "You are a coding assistant.".to_owned(),
        }
    }

    #[test]
    fn definition_rejects_duplicate_tools() {
        let mut value = definition("v1");
        value.tools.push(ToolName::new("filesystem"));

        assert!(matches!(
            validate_definition(&value),
            Err(StudioError::InvalidInput { field: "tools" })
        ));
    }
}
