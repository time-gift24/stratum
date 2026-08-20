//! Container-backed verification that Studio PostgreSQL is the sole runtime
//! source for Provider, Model, credential, and Agent authoring definitions.

use crate::integration_common as common;

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header::ETAG},
};
use common::{
    corrupt_studio_pg_url, ontology_pg_url, pg_url, postcommit_studio_pg_url, reset_studio,
    studio_pg_url,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use stratum_api::{AppState, HostError, router};
use stratum_config::Config;
use stratum_core::{AgentVersionTag, ModelConfig, ModelId, ToolName};
use stratum_ontology::OntologyStore;
use stratum_postgres::PostgresBackend;
use stratum_studio::{AgentDefinitionInput, ProviderKind, StudioError, StudioStore};
use tower::ServiceExt;

const SECRET_SENTINEL: &str = "studio-db-only-secret-sentinel";
const MANAGEMENT_SECRET: &str = "management-create-secret-sentinel";
const MANAGEMENT_REPLACEMENT_SECRET: &str = "management-replacement-secret-sentinel";
const MODEL_NAME: &str = "db-only-model";
const MODEL_ID: &str = "openai:db-only-model";
const MANAGEMENT_MODEL_NAME: &str = "management-model";
const MANAGEMENT_MODEL_ID: &str = "openai:management-model";
const MANAGEMENT_AGENT_NAME: &str = "management-agent";

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn studio_database_is_the_complete_runtime_catalog() {
    let studio = StudioStore::connect(&studio_pg_url())
        .await
        .expect("Studio PostgreSQL test database connects");
    reset_studio(&studio).await;

    // An empty Studio catalog is a valid runtime state. Compatibility routes
    // project the empty database even when management routes are hidden.
    let disabled = assembled_app(studio.clone(), false).await;
    let (status, disabled_models) = json_request(&disabled, "GET", "/v1/models", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled_models, json!({ "models": [] }));
    let (status, disabled_templates) =
        json_request(&disabled, "GET", "/v1/agent-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled_templates, json!({ "templates": [] }));
    let (status, _) = json_request(&disabled, "GET", "/v1/providers", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, disabled_openapi) =
        json_request(&disabled, "GET", "/api-docs/openapi.json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(disabled_openapi["paths"].get("/v1/providers").is_none());
    assert!(
        disabled_openapi["paths"]
            .get("/v1/agent-definitions")
            .is_none()
    );

    // Enabling management changes only route exposure, not the runtime
    // catalog projected from the same Studio database.
    let enabled_empty = assembled_app(studio.clone(), true).await;
    let (status, enabled_models) = json_request(&enabled_empty, "GET", "/v1/models", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled_models, disabled_models);
    let (status, enabled_templates) =
        json_request(&enabled_empty, "GET", "/v1/agent-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled_templates, disabled_templates);
    let (status, providers) = json_request(&enabled_empty, "GET", "/v1/providers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers["data"], json!([]));
    let (status, enabled_openapi) =
        json_request(&enabled_empty, "GET", "/api-docs/openapi.json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(enabled_openapi["paths"]["/v1/providers"].is_object());
    assert!(enabled_openapi["paths"]["/v1/agent-definitions"].is_object());

    // Populate only Studio, then assemble a fresh process view. No config or
    // filesystem source participates in the resulting registry/projection.
    studio
        .create_provider(
            ProviderKind::Openai,
            SecretString::from(SECRET_SENTINEL.to_owned()),
        )
        .await
        .expect("Studio Provider is created");
    studio
        .create_model(ProviderKind::Openai, MODEL_NAME.to_owned())
        .await
        .expect("Studio model is created");
    studio
        .create_agent_definition(AgentDefinitionInput {
            agent_name: "db-agent".parse().expect("static Agent name is valid"),
            agent_version: AgentVersionTag::new("db-v1").expect("static Agent version is valid"),
            model: ModelConfig::new(
                MODEL_ID
                    .parse::<ModelId>()
                    .expect("static Model id is valid"),
                serde_json::Map::new(),
            ),
            tools: vec![ToolName::from("echo")],
            prompt: "Use only the Studio-backed definition.".to_owned(),
        })
        .await
        .expect("Studio Agent definition is created");

    // The already-assembled production state must observe the committed DB
    // catalog directly. There is no process-local refresh window or stale
    // revision that can overwrite a newer Studio write.
    let (status, hot_models) = json_request(&enabled_empty, "GET", "/v1/models", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hot_models["models"][0]["model"], MODEL_ID);
    let (status, hot_templates) =
        json_request(&enabled_empty, "GET", "/v1/agent-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hot_templates["templates"][0]["agent_name"], "db-agent");

    let populated = assembled_app(studio, true).await;
    let (status, models) = json_request(&populated, "GET", "/v1/models", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        models,
        json!({
            "models": [{
                "model": MODEL_ID,
                "parameters_schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "default": {},
                },
            }],
        })
    );
    let (status, templates) = json_request(&populated, "GET", "/v1/agent-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        templates,
        json!({
            "templates": [{
                "agent_name": "db-agent",
                "version": "db-v1",
                "model_config": { "model": MODEL_ID, "parameters": {} },
            }],
        })
    );

    // Read APIs and error envelopes expose only credential presence. Even a
    // request containing the secret must not receive it back on conflict.
    let (status, providers) = json_request(&populated, "GET", "/v1/providers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers["data"][0]["provider"], "openai");
    assert_eq!(providers["data"][0]["credential_configured"], true);
    assert!(!providers.to_string().contains(SECRET_SENTINEL));
    let (status, conflict) = json_request(
        &populated,
        "POST",
        "/v1/providers",
        Some(json!({ "provider": "openai", "api_key": SECRET_SENTINEL })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(!conflict.to_string().contains(SECRET_SENTINEL));
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn management_http_flow_is_versioned_blocking_persistent_and_secret_safe() {
    let studio = StudioStore::connect(&studio_pg_url())
        .await
        .expect("Studio PostgreSQL test database connects");
    reset_studio(&studio).await;
    let app = assembled_app(studio.clone(), true).await;

    let (status, provider_headers, provider) = json_request_with_if_match(
        &app,
        "POST",
        "/v1/providers",
        Some(json!({
            "provider": "openai",
            "api_key": MANAGEMENT_SECRET,
        })),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "unexpected response: {provider}"
    );
    assert_eq!(provider["provider"], "openai");
    assert_eq!(provider["credential_configured"], true);
    assert_eq!(provider["models_count"], 0);
    assert_secrets_absent(&provider);
    let created_provider_etag = response_etag(&provider_headers);

    let (status, read_provider_headers, read_provider) =
        json_request_with_if_match(&app, "GET", "/v1/providers/openai", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response_etag(&read_provider_headers), created_provider_etag);
    assert_eq!(read_provider["provider"], "openai");
    assert_secrets_absent(&read_provider);

    let (status, duplicate) = json_request(
        &app,
        "POST",
        "/v1/providers",
        Some(json!({
            "provider": "openai",
            "api_key": MANAGEMENT_SECRET,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(duplicate["error"]["code"], "studio_conflict");
    assert_secrets_absent(&duplicate);

    let (status, updated_provider_headers, updated_provider) = json_request_with_if_match(
        &app,
        "PUT",
        "/v1/providers/openai",
        Some(json!({ "api_key": MANAGEMENT_REPLACEMENT_SECRET })),
        Some(&created_provider_etag),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected response: {updated_provider}"
    );
    let updated_provider_etag = response_etag(&updated_provider_headers);
    assert_ne!(updated_provider_etag, created_provider_etag);
    assert_secrets_absent(&updated_provider);

    let (status, _, stale_provider) = json_request_with_if_match(
        &app,
        "PUT",
        "/v1/providers/openai",
        Some(json!({ "api_key": MANAGEMENT_SECRET })),
        Some(&created_provider_etag),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        stale_provider["error"]["code"],
        "studio_precondition_failed"
    );
    assert_secrets_absent(&stale_provider);

    let (status, model_headers, model) = json_request_with_if_match(
        &app,
        "POST",
        "/v1/providers/openai/models",
        Some(json!({ "name": MANAGEMENT_MODEL_NAME })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected response: {model}");
    assert_eq!(model["model_id"], MANAGEMENT_MODEL_ID);
    assert_eq!(model["provider"], "openai");
    assert_eq!(model["name"], MANAGEMENT_MODEL_NAME);
    let model_etag = response_etag(&model_headers);

    let (status, models) = json_request(
        &app,
        "GET",
        "/v1/providers/openai/models?page=1&per_page=20",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models["pagination"]["total"], 1);
    assert_eq!(models["data"][0]["model_id"], MANAGEMENT_MODEL_ID);

    let (status, read_model_headers, read_model) = json_request_with_if_match(
        &app,
        "GET",
        "/v1/providers/openai/models/management-model",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response_etag(&read_model_headers), model_etag);
    assert_eq!(read_model["model_id"], MANAGEMENT_MODEL_ID);

    let (status, agent_headers, agent) = json_request_with_if_match(
        &app,
        "POST",
        "/v1/agent-definitions",
        Some(json!({
            "agent_name": MANAGEMENT_AGENT_NAME,
            "agent_version": "management-v1",
            "model": MANAGEMENT_MODEL_ID,
            "model_parameters": {},
            "tools": ["echo"],
            "prompt": "Use the managed model.",
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "unexpected response: {agent}");
    assert_eq!(agent["agent_name"], MANAGEMENT_AGENT_NAME);
    assert_eq!(agent["agent_version"], "management-v1");
    let created_agent_etag = response_etag(&agent_headers);

    let (status, agents) = json_request(
        &app,
        "GET",
        "/v1/agent-definitions?sort=-updated_at&page=1&per_page=20",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(agents["pagination"]["total"], 1);
    assert_eq!(agents["data"][0]["agent_name"], MANAGEMENT_AGENT_NAME);

    let (status, read_agent_headers, read_agent) = json_request_with_if_match(
        &app,
        "GET",
        "/v1/agent-definitions/management-agent",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response_etag(&read_agent_headers), created_agent_etag);
    assert_eq!(read_agent["prompt"], "Use the managed model.");

    let (status, updated_agent_headers, updated_agent) = json_request_with_if_match(
        &app,
        "PUT",
        "/v1/agent-definitions/management-agent",
        Some(json!({
            "agent_version": "management-v2",
            "model": MANAGEMENT_MODEL_ID,
            "model_parameters": {},
            "tools": ["echo"],
            "prompt": "Use the persisted managed model.",
        })),
        Some(&created_agent_etag),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "unexpected response: {updated_agent}"
    );
    assert_eq!(updated_agent["agent_version"], "management-v2");
    let updated_agent_etag = response_etag(&updated_agent_headers);
    assert_ne!(updated_agent_etag, created_agent_etag);

    let (status, _, blocked_model) = json_request_with_if_match(
        &app,
        "DELETE",
        "/v1/providers/openai/models/management-model",
        None,
        Some(&model_etag),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(blocked_model["error"]["code"], "studio_conflict");
    assert_eq!(
        blocked_model["error"]["blockers"],
        json!([{
            "resource_type": "agent_definition",
            "name": MANAGEMENT_AGENT_NAME,
        }])
    );

    let (_, current_provider_headers, _) =
        json_request_with_if_match(&app, "GET", "/v1/providers/openai", None, None).await;
    let current_provider_etag = response_etag(&current_provider_headers);
    let (status, _, blocked_provider) = json_request_with_if_match(
        &app,
        "DELETE",
        "/v1/providers/openai",
        None,
        Some(&current_provider_etag),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(blocked_provider["error"]["code"], "studio_conflict");
    assert_eq!(
        blocked_provider["error"]["blockers"],
        json!([{
            "resource_type": "agent_definition",
            "name": MANAGEMENT_AGENT_NAME,
        }])
    );

    let (status, compatibility_models) = json_request(&app, "GET", "/v1/models", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        compatibility_models["models"][0]["model"],
        MANAGEMENT_MODEL_ID
    );
    let (status, compatibility_templates) =
        json_request(&app, "GET", "/v1/agent-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        compatibility_templates["templates"][0]["agent_name"],
        MANAGEMENT_AGENT_NAME
    );
    assert_eq!(
        compatibility_templates["templates"][0]["version"],
        "management-v2"
    );

    // A fresh AppState over the same Store observes the committed rows without
    // any config/template fallback or in-memory catalog restoration.
    let restarted = assembled_app(studio, true).await;
    let (status, restarted_provider) =
        json_request(&restarted, "GET", "/v1/providers/openai", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restarted_provider["models_count"], 1);
    assert_secrets_absent(&restarted_provider);
    let (status, restarted_model) = json_request(
        &restarted,
        "GET",
        "/v1/providers/openai/models/management-model",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restarted_model["model_id"], MANAGEMENT_MODEL_ID);
    let (status, restarted_agent) = json_request(
        &restarted,
        "GET",
        "/v1/agent-definitions/management-agent",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restarted_agent["agent_version"], "management-v2");

    let (status, _, _) = json_request_with_if_match(
        &restarted,
        "DELETE",
        "/v1/agent-definitions/management-agent",
        None,
        Some(&updated_agent_etag),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = json_request_with_if_match(
        &restarted,
        "DELETE",
        "/v1/providers/openai/models/management-model",
        None,
        Some(&model_etag),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, provider_after_model_headers, _) =
        json_request_with_if_match(&restarted, "GET", "/v1/providers/openai", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let provider_after_model_etag = response_etag(&provider_after_model_headers);
    let (status, _, _) = json_request_with_if_match(
        &restarted,
        "DELETE",
        "/v1/providers/openai",
        None,
        Some(&provider_after_model_etag),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = json_request(&restarted, "GET", "/v1/providers/openai", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = json_request(
        &restarted,
        "GET",
        "/v1/providers/openai/models/management-model",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = json_request(
        &restarted,
        "GET",
        "/v1/agent-definitions/management-agent",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, models_after_cleanup) = json_request(&restarted, "GET", "/v1/models", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models_after_cleanup, json!({ "models": [] }));
    let (status, templates_after_cleanup) =
        json_request(&restarted, "GET", "/v1/agent-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(templates_after_cleanup, json!({ "templates": [] }));
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn model_message_test_enforces_catalog_membership_before_any_upstream_call() {
    let studio = StudioStore::connect(&studio_pg_url())
        .await
        .expect("Studio PostgreSQL test database connects");
    reset_studio(&studio).await;
    let app = assembled_app(studio.clone(), true).await;

    // A missing Provider stays local and never reaches the upstream adapter.
    let (status, missing_provider) = json_request(
        &app,
        "POST",
        "/v1/providers/openai/models/any-model/test",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing_provider["error"]["code"], "provider_not_found");

    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/providers",
        Some(json!({
            "provider": "openai",
            "api_key": MANAGEMENT_SECRET,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // A model outside the Provider catalog is a managed-model miss and never
    // reaches the upstream adapter. Transport success/failure for a
    // configured model is covered by loopback unit tests in the API host.
    let (status, unknown_model) = json_request(
        &app,
        "POST",
        "/v1/providers/openai/models/unknown-model/test",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(unknown_model["error"]["code"], "managed_model_not_found");
    assert_secrets_absent(&unknown_model);
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn missing_studio_credential_fails_runtime_assembly_closed() {
    let studio = StudioStore::connect(&corrupt_studio_pg_url())
        .await
        .expect("deliberately corrupt Studio database migrates");
    let pg = PostgresBackend::connect(&pg_url())
        .await
        .expect("execution PostgreSQL connects");
    let ontology = OntologyStore::connect(&ontology_pg_url())
        .await
        .expect("Ontology PostgreSQL connects");
    let config = config(&corrupt_studio_pg_url(), false);

    let error = AppState::with_studio(pg, None, ontology, config, studio)
        .await
        .err()
        .expect("corrupt Studio catalog must fail assembly");

    assert!(
        matches!(
            &error,
            HostError::Studio(StudioError::CatalogCorrupt {
                field: "provider_credential"
            })
        ),
        "missing credentials must fail runtime assembly closed: {error:?}"
    );
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn committed_model_create_has_no_postcommit_catalog_failure() {
    let studio = StudioStore::connect(&postcommit_studio_pg_url())
        .await
        .expect("fault-injection Studio database migrates");
    reset_studio(&studio).await;
    studio
        .create_provider(
            ProviderKind::Openai,
            SecretString::from("postcommit-test-key"),
        )
        .await
        .expect("fault-injection Provider is restored");
    let app = assembled_app_with_url(studio.clone(), &postcommit_studio_pg_url(), true).await;

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/providers/openai/models",
        Some(json!({ "name": "atomic-model" })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "unexpected response: {body}");
    assert_eq!(body["model_id"], "openai:atomic-model");
    assert_eq!(body["parameter_schema"]["default"], json!({}));
    assert_eq!(
        studio
            .model(ProviderKind::Openai, "atomic-model")
            .await
            .expect("committed Model remains readable")
            .value
            .model
            .as_str(),
        "openai:atomic-model"
    );
}

async fn assembled_app(studio: StudioStore, management_enabled: bool) -> Router {
    assembled_app_with_url(studio, &studio_pg_url(), management_enabled).await
}

async fn assembled_app_with_url(
    studio: StudioStore,
    studio_url: &str,
    management_enabled: bool,
) -> Router {
    let pg = PostgresBackend::connect(&pg_url())
        .await
        .expect("execution PostgreSQL connects");
    let ontology = OntologyStore::connect(&ontology_pg_url())
        .await
        .expect("Ontology PostgreSQL connects");
    let state = AppState::with_studio(
        pg,
        None,
        ontology,
        config(studio_url, management_enabled),
        studio,
    )
    .await
    .expect("DB-only state assembles");
    router(Arc::new(state))
}

fn config(studio_url: &str, management_enabled: bool) -> Config {
    Config::parse(&format!(
        r#"
[api]
bind = "127.0.0.1:0"

[postgres]
url = {pg_url:?}

[ontology]
database_url = {ontology_url:?}

[studio]
database_url = {studio_url:?}
management_enabled = {management_enabled}
"#,
        pg_url = pg_url(),
        ontology_url = ontology_pg_url(),
    ))
    .expect("DB-only test config parses")
}

async fn json_request(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, body) = json_request_with_if_match(app, method, uri, body, None).await;
    (status, body)
}

async fn json_request_with_if_match(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    if_match: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(if_match) = if_match {
        request = request.header("if-match", if_match);
    }
    let response = app
        .clone()
        .oneshot(request.body(body).expect("request builds"))
        .await
        .expect("router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("response body is JSON")
    };
    (status, headers, value)
}

fn response_etag(headers: &HeaderMap) -> String {
    headers
        .get(ETAG)
        .expect("versioned response has an ETag")
        .to_str()
        .expect("ETag is valid UTF-8")
        .to_owned()
}

fn assert_secrets_absent(value: &Value) {
    let encoded = value.to_string();
    assert!(!encoded.contains(MANAGEMENT_SECRET));
    assert!(!encoded.contains(MANAGEMENT_REPLACEMENT_SECRET));
}
