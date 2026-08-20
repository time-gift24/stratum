use secrecy::SecretString;
use serde_json::Map;
use sqlx::PgPool;
use stratum_core::{AgentName, AgentVersionTag, ModelConfig, ModelId};
use stratum_studio::{AgentDefinitionInput, ProviderKind, StudioError, StudioStore};

const DATABASE_URL_ENV: &str = "STRATUM_STUDIO_TEST_DATABASE_URL";

#[tokio::test]
#[ignore = "requires the stratum-studio PostgreSQL test container"]
async fn provider_delete_removes_owned_rows_but_blocks_agent_references() {
    let database_url = std::env::var(DATABASE_URL_ENV).expect("test database URL is configured");
    let store = StudioStore::connect(&database_url)
        .await
        .expect("Studio migrations apply");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("test cleanup pool connects");
    reset_catalog(&pool).await;

    store
        .create_provider(ProviderKind::Openai, SecretString::from("test-key"))
        .await
        .expect("provider is created");
    store
        .create_model(ProviderKind::Openai, "gpt-5".to_owned())
        .await
        .expect("first model is created");
    store
        .create_model(ProviderKind::Openai, "gpt-5-mini".to_owned())
        .await
        .expect("second model is created");
    let provider = store
        .provider(ProviderKind::Openai)
        .await
        .expect("provider version is current");

    store
        .delete_provider(ProviderKind::Openai, provider.version)
        .await
        .expect("unreferenced models are deleted with their provider");

    assert!(matches!(
        store.provider(ProviderKind::Openai).await,
        Err(StudioError::NotFound)
    ));
    assert!(
        store
            .list_models()
            .await
            .expect("models list loads")
            .is_empty()
    );
    assert_eq!(credential_count(&pool).await, 0);

    store
        .create_provider(
            ProviderKind::Deepseek,
            SecretString::from("another-test-key"),
        )
        .await
        .expect("provider is created");
    store
        .create_model(ProviderKind::Deepseek, "deepseek-chat".to_owned())
        .await
        .expect("model is created");
    let agent_name = AgentName::new("referencing-agent").expect("agent name is valid");
    store
        .create_agent_definition(AgentDefinitionInput {
            agent_name: agent_name.clone(),
            agent_version: AgentVersionTag::new("v1").expect("version is valid"),
            model: ModelConfig::new(
                ModelId::new("deepseek", "deepseek-chat").expect("model id is valid"),
                Map::new(),
            ),
            tools: Vec::new(),
            prompt: "Answer carefully.".to_owned(),
        })
        .await
        .expect("agent definition is created");
    let provider = store
        .provider(ProviderKind::Deepseek)
        .await
        .expect("provider version is current");

    let error = store
        .delete_provider(ProviderKind::Deepseek, provider.version)
        .await
        .expect_err("agent reference blocks provider deletion");

    let StudioError::DeletionBlocked { blockers } = error else {
        panic!("expected deletion blockers, got {error}");
    };
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0].resource, "agent_definition");
    assert_eq!(blockers[0].name, agent_name.as_str());
    assert_eq!(
        store
            .provider(ProviderKind::Deepseek)
            .await
            .expect("blocked provider remains")
            .value
            .models_count,
        1
    );
    assert_eq!(credential_count(&pool).await, 1);
}

#[tokio::test]
#[ignore = "requires the stratum-studio PostgreSQL test container"]
async fn corrupt_created_representation_rolls_back_the_mutation() {
    let database_url = std::env::var(DATABASE_URL_ENV).expect("test database URL is configured");
    let store = StudioStore::connect(&database_url)
        .await
        .expect("Studio migrations apply");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("test fault-injection pool connects");
    reset_catalog(&pool).await;
    configure_openai_model(&store).await;
    install_corrupt_agent_parameters_trigger(&pool).await;

    let agent_name = AgentName::new("corrupt-created-agent").expect("agent name is valid");
    let error = store
        .create_agent_definition(agent_definition(&agent_name, "v1", "original prompt"))
        .await
        .expect_err("corrupt representation must fail the create");
    remove_corrupt_agent_parameters_trigger(&pool).await;

    assert!(matches!(
        error,
        StudioError::CatalogCorrupt {
            field: "model_parameters"
        }
    ));
    assert!(matches!(
        store.agent_definition(&agent_name).await,
        Err(StudioError::NotFound)
    ));
}

#[tokio::test]
#[ignore = "requires the stratum-studio PostgreSQL test container"]
async fn corrupt_updated_representation_preserves_the_previous_revision() {
    let database_url = std::env::var(DATABASE_URL_ENV).expect("test database URL is configured");
    let store = StudioStore::connect(&database_url)
        .await
        .expect("Studio migrations apply");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("test fault-injection pool connects");
    reset_catalog(&pool).await;
    configure_openai_model(&store).await;
    let agent_name = AgentName::new("corrupt-updated-agent").expect("agent name is valid");
    let original = store
        .create_agent_definition(agent_definition(&agent_name, "v1", "original prompt"))
        .await
        .expect("original Agent definition is created");
    install_corrupt_agent_parameters_trigger(&pool).await;

    let error = store
        .replace_agent_definition(
            agent_definition(&agent_name, "v2", "replacement prompt"),
            original.version,
        )
        .await
        .expect_err("corrupt representation must fail the replacement");
    remove_corrupt_agent_parameters_trigger(&pool).await;
    let persisted = store
        .agent_definition(&agent_name)
        .await
        .expect("the previous Agent definition remains readable");

    assert!(matches!(
        error,
        StudioError::CatalogCorrupt {
            field: "model_parameters"
        }
    ));
    assert_eq!(persisted.version, original.version);
    assert_eq!(persisted.value.agent_version.as_str(), "v1");
    assert_eq!(persisted.value.prompt, "original prompt");
}

#[tokio::test]
#[ignore = "requires the stratum-studio PostgreSQL test container"]
async fn database_rejects_agent_version_tags_outside_the_newtype_boundary() {
    let database_url = std::env::var(DATABASE_URL_ENV).expect("test database URL is configured");
    let _store = StudioStore::connect(&database_url)
        .await
        .expect("Studio migrations apply");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("test constraint pool connects");
    reset_catalog(&pool).await;

    let invalid_versions = [
        String::new(),
        " leading".to_owned(),
        "trailing ".to_owned(),
        "control\ncharacter".to_owned(),
        "x".repeat(129),
    ];
    for (index, version) in invalid_versions.iter().enumerate() {
        let result = sqlx::query(
            "INSERT INTO studio_agent_definitions \
             (agent_name, version, model_id, model_parameters, tools, prompt, revision) \
             VALUES ($1, $2, 'openai:gpt-5', '{}'::jsonb, '[]'::jsonb, \
                     'valid prompt', gen_random_uuid())",
        )
        .bind(format!("invalid-version-{index}"))
        .bind(version)
        .execute(&pool)
        .await;

        assert!(result.is_err(), "database accepted invalid version {index}");
    }

    sqlx::query(
        "INSERT INTO studio_agent_definitions \
         (agent_name, version, model_id, model_parameters, tools, prompt, revision) \
         VALUES ('valid-version', 'Release-α', 'openai:gpt-5', '{}'::jsonb, \
                 '[]'::jsonb, 'valid prompt', gen_random_uuid())",
    )
    .execute(&pool)
    .await
    .expect("database accepts a valid exact author tag");
}

#[tokio::test]
#[ignore = "requires the stratum-studio PostgreSQL test container"]
async fn provider_reads_fail_closed_when_the_credential_row_is_missing() {
    let database_url = std::env::var(DATABASE_URL_ENV).expect("test database URL is configured");
    let store = StudioStore::connect(&database_url)
        .await
        .expect("Studio migrations apply");
    let pool = PgPool::connect(&database_url)
        .await
        .expect("test corruption pool connects");
    reset_catalog(&pool).await;
    store
        .create_provider(ProviderKind::Openai, SecretString::from("test-key"))
        .await
        .expect("provider is created");
    sqlx::query("DELETE FROM studio_provider_credentials WHERE provider_kind = 'openai'")
        .execute(&pool)
        .await
        .expect("test removes the credential row");

    let runtime_error = match store.runtime_providers().await {
        Ok(_) => panic!("runtime catalog accepted a missing credential"),
        Err(error) => error,
    };
    for error in [
        store
            .provider(ProviderKind::Openai)
            .await
            .expect_err("single Provider read fails closed"),
        store
            .list_providers()
            .await
            .expect_err("Provider list fails closed"),
        runtime_error,
    ] {
        assert!(matches!(
            error,
            StudioError::CatalogCorrupt {
                field: "provider_credential"
            }
        ));
    }
}

async fn configure_openai_model(store: &StudioStore) {
    store
        .create_provider(ProviderKind::Openai, SecretString::from("test-key"))
        .await
        .expect("provider is created");
    store
        .create_model(ProviderKind::Openai, "gpt-5".to_owned())
        .await
        .expect("model is created");
}

fn agent_definition(agent_name: &AgentName, version: &str, prompt: &str) -> AgentDefinitionInput {
    AgentDefinitionInput {
        agent_name: agent_name.clone(),
        agent_version: AgentVersionTag::new(version).expect("version is valid"),
        model: ModelConfig::new(
            ModelId::new("openai", "gpt-5").expect("model id is valid"),
            Map::new(),
        ),
        tools: Vec::new(),
        prompt: prompt.to_owned(),
    }
}

async fn install_corrupt_agent_parameters_trigger(pool: &PgPool) {
    sqlx::query(
        "CREATE OR REPLACE FUNCTION corrupt_agent_parameters() RETURNS trigger \
         LANGUAGE plpgsql AS $$ BEGIN NEW.model_parameters = '[]'::jsonb; RETURN NEW; END; $$",
    )
    .execute(pool)
    .await
    .expect("fault-injection function is installed");
    sqlx::query(
        "CREATE TRIGGER corrupt_agent_parameters_before_write \
         BEFORE INSERT OR UPDATE ON studio_agent_definitions \
         FOR EACH ROW EXECUTE FUNCTION corrupt_agent_parameters()",
    )
    .execute(pool)
    .await
    .expect("fault-injection trigger is installed");
}

async fn remove_corrupt_agent_parameters_trigger(pool: &PgPool) {
    sqlx::query("DROP TRIGGER corrupt_agent_parameters_before_write ON studio_agent_definitions")
        .execute(pool)
        .await
        .expect("fault-injection trigger is removed");
    sqlx::query("DROP FUNCTION corrupt_agent_parameters()")
        .execute(pool)
        .await
        .expect("fault-injection function is removed");
}

async fn reset_catalog(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE studio_agent_definitions, studio_models, \
         studio_provider_credentials, studio_providers",
    )
    .execute(pool)
    .await
    .expect("test catalog is reset");
}

async fn credential_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM studio_provider_credentials")
        .fetch_one(pool)
        .await
        .expect("row count loads")
}
