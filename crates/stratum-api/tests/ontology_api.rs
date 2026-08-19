//! Container-backed HTTP verification for canonical Ontology metadata.
//!
//! Requires this crate's PostgreSQL test container.

use std::{fs, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{
        HeaderMap, HeaderValue, Method, Request, StatusCode,
        header::{CONTENT_TYPE, ETAG, IF_MATCH, LOCATION},
    },
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};
use stratum_api::{ApiError, AppState, HostError, router, run_from_path};
use stratum_config::{Config, ConfigError};
use stratum_core::{ModelConfig, ModelId};
use stratum_llm::{
    ChatRequest, ChatResponse, ChatStream, ConfigurableLlmProvider, LlmError, LlmProvider,
    LlmProviderManager,
};
use stratum_ontology::{OntologyStore, OntologyStoreError};
use stratum_postgres::PostgresBackend;
use stratum_studio::StudioStore;
use tower::ServiceExt;
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str =
    "postgres://stratum:stratum@127.0.0.1:45433/stratum_ontology_test";

struct OntologyFixture {
    app: Router,
}

impl OntologyFixture {
    async fn new() -> Self {
        let database_url = database_url();
        let execution_database_url = execution_database_url();
        let studio_database_url = studio_database_url();
        let config = Config::parse(&format!(
            r#"
[api]
bind = "127.0.0.1:0"
allowed_origins = ["http://localhost:5173"]
readiness_timeout_ms = 1000

[ontology]
database_url = {database_url:?}

[postgres]
url = {execution_database_url:?}

[studio]
database_url = {studio_database_url:?}
management_enabled = false
"#,
        ))
        .expect("test configuration parses");
        let model = ModelId::new("openai", "test-model").expect("test model is valid");
        let mut providers = LlmProviderManager::new();
        providers
            .register(Arc::new(TestProvider(model)))
            .expect("test provider registers");
        let pg = PostgresBackend::connect(&execution_database_url)
            .await
            .expect("execution PostgreSQL test container is available");
        let store = OntologyStore::connect(&database_url)
            .await
            .expect("PostgreSQL test container is available");
        let studio = StudioStore::connect(&studio_database_url)
            .await
            .expect("Studio PostgreSQL test database is available");
        let state = Arc::new(
            AppState::new(pg, None, providers, store, studio, config)
                .await
                .expect("state assembles"),
        );
        let app = router(state);

        Self { app }
    }

    async fn request(&self, request: Request<Body>) -> Response {
        self.app
            .clone()
            .oneshot(request)
            .await
            .expect("router completes request")
    }

    async fn create(&self, name: &str, description: Option<&str>) -> CreatedOntology {
        let mut payload = json!({
            "name": name,
            "display_name": "HTTP verification Ontology",
        });
        if let Some(description) = description {
            payload["description"] = json!(description);
        }
        let (status, headers, body) = response_json(
            self.request(json_request(Method::POST, "/v1/ontologies", payload))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = string_field(&body, "id");
        let etag = header_value(&headers, ETAG);
        assert_eq!(
            header_value(&headers, LOCATION),
            format!("/v1/ontologies/{id}")
        );
        assert!(
            etag.starts_with(&format!("\"ontology:{id}:")) && etag.ends_with("\""),
            "ETag must use the documented strong canonical form: {etag}"
        );
        CreatedOntology { id, etag, body }
    }
}

struct CreatedOntology {
    id: String,
    etag: String,
    body: Value,
}

#[derive(Clone)]
struct TestProvider(ModelId);

#[async_trait]
impl LlmProvider for TestProvider {
    fn model_id(&self) -> ModelId {
        self.0.clone()
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::MockExhausted)
    }

    async fn chat_stream(&self, _request: ChatRequest) -> Result<ChatStream, LlmError> {
        Err(LlmError::MockExhausted)
    }
}

impl ConfigurableLlmProvider for TestProvider {
    fn parameter_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false, "default": {}})
    }

    fn default_model_config(&self) -> ModelConfig {
        ModelConfig::new(self.model_id(), Map::new())
    }

    fn configure(&self, parameters: &Map<String, Value>) -> Result<Arc<dyn LlmProvider>, LlmError> {
        if parameters.is_empty() {
            Ok(Arc::new(self.clone()))
        } else {
            Err(LlmError::InvalidModelParameters {
                model: self.model_id(),
            })
        }
    }
}

fn database_url() -> String {
    std::env::var("STRATUM_API_TEST_ONTOLOGY_PG_URL")
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
}

fn execution_database_url() -> String {
    std::env::var("STRATUM_API_TEST_PG_URL")
        .unwrap_or_else(|_| "postgres://stratum:stratum@127.0.0.1:45433/stratum_test".to_owned())
}

fn studio_database_url() -> String {
    std::env::var("STRATUM_API_TEST_STUDIO_PG_URL").unwrap_or_else(|_| {
        "postgres://stratum:stratum@127.0.0.1:45433/stratum_studio_test".to_owned()
    })
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::now_v7().simple())
}

fn json_request(method: Method, uri: impl AsRef<str>, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("JSON request builds")
}

fn json_request_with_etag(
    method: Method,
    uri: impl AsRef<str>,
    body: Value,
    etag: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header(CONTENT_TYPE, "application/json")
        .header(IF_MATCH, etag)
        .body(Body::from(body.to_string()))
        .expect("conditional JSON request builds")
}

fn empty_request_with_etag(method: Method, uri: impl AsRef<str>, etag: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header(IF_MATCH, etag)
        .body(Body::empty())
        .expect("conditional empty request builds")
}

async fn response_json(response: Response) -> (StatusCode, HeaderMap, Value) {
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body is readable");
    let body = serde_json::from_slice(&body).expect("response body is JSON");
    (status, headers, body)
}

async fn response_bytes(response: Response) -> (StatusCode, HeaderMap, Vec<u8>) {
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body is readable")
        .to_vec();
    (status, headers, body)
}

fn header_value(headers: &HeaderMap, name: axum::http::header::HeaderName) -> String {
    headers
        .get(name)
        .expect("required response header is present")
        .to_str()
        .expect("response header is valid ASCII")
        .to_owned()
}

fn string_field(value: &Value, name: &str) -> String {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("{name} is a string"))
        .to_owned()
}

fn assert_error(body: &Value, code: &str) {
    let error = body["error"]
        .as_object()
        .expect("error envelope is present");
    assert_eq!(error.get("code").and_then(Value::as_str), Some(code));
    assert!(
        error.get("message").is_some_and(Value::is_string),
        "error envelope has a safe message"
    );
    assert!(
        error.get("violations").is_none(),
        "only Ontology 422 responses may expose violations"
    );
}

#[derive(Debug, Clone)]
struct CandidateIds {
    person: String,
    company: String,
}

fn candidate(id: &str, name: &str) -> (Value, CandidateIds) {
    let person = Uuid::now_v7().to_string();
    let company = Uuid::now_v7().to_string();
    let person_email = Uuid::now_v7().to_string();
    let person_name = Uuid::now_v7().to_string();
    let company_name = Uuid::now_v7().to_string();
    let employs = Uuid::now_v7().to_string();
    let knows = Uuid::now_v7().to_string();
    let document = json!({
        "id": id,
        "name": name,
        "display_name": "Full HTTP document",
        "description": "Root description",
        "object_types": [
            {
                "id": person,
                "name": "person",
                "display_name": "Person",
                "description": "A person",
                "properties": [
                    {
                        "id": person_email,
                        "name": "email",
                        "display_name": "Email",
                        "description": "A mail address",
                        "value_type": "string",
                        "required": true
                    },
                    {
                        "id": person_name,
                        "name": "name",
                        "display_name": "Name",
                        "value_type": "string",
                        "required": true
                    }
                ]
            },
            {
                "id": company,
                "name": "company",
                "display_name": "Company",
                "properties": [
                    {
                        "id": company_name,
                        "name": "name",
                        "display_name": "Name",
                        "value_type": "string",
                        "required": false
                    }
                ]
            }
        ],
        "link_types": [
            {
                "id": employs,
                "name": "employs",
                "display_name": "Employs",
                "description": "Company employs person",
                "source_object_type_id": company,
                "target_object_type_id": person,
                "source_to_target": "many",
                "target_to_source": "one"
            },
            {
                "id": knows,
                "name": "knows",
                "display_name": "Knows",
                "source_object_type_id": person,
                "target_object_type_id": person,
                "source_to_target": "many",
                "target_to_source": "many"
            }
        ],
        "canvas": {
            "positions": [
                {"object_type_id": company, "x": 32.0, "y": 16.0},
                {"object_type_id": person, "x": 8.0, "y": 24.0}
            ]
        }
    });
    (document, CandidateIds { person, company })
}

fn expected_neighborhood(document: &Value, origin_object_type_id: &str) -> Value {
    json!({
        "origin_object_type_id": origin_object_type_id,
        "depth": 1,
        "object_types": document["object_types"].clone(),
        "link_types": document["link_types"].clone(),
        "canvas": document["canvas"].clone(),
    })
}

#[tokio::test]
async fn api_startup_rejects_a_missing_ontology_section_before_external_initialization() {
    let root =
        std::env::temp_dir().join(format!("stratum-api-missing-ontology-{}", Uuid::now_v7()));
    fs::create_dir_all(&root).expect("temporary root is created");
    let path = root.join("stratum.toml");
    fs::write(
        &path,
        r#"
[api]
bind = "127.0.0.1:0"
readiness_timeout_ms = 1000

[nats]
url = "nats://127.0.0.1:4222"
stream_name = "stratum"
subject_prefix = "stratum"
replicas = 1
max_age_seconds = 60
max_bytes = 1024
max_messages = 100

[postgres]
url = "postgres://unused:unused@127.0.0.1:1/unused"

[studio]
database_url = "postgres://unused:unused@127.0.0.1:1/unused_studio"
"#,
    )
    .expect("test config is written");

    let error = run_from_path(&path)
        .await
        .expect_err("missing ontology must fail before opening external connections");

    assert!(
        matches!(
            error,
            HostError::Config(ConfigError::MissingSection {
                section: "ontology"
            })
        ),
        "unexpected startup error: {error:?}"
    );
    fs::remove_dir_all(root).expect("temporary root is removed");
}

#[tokio::test]
async fn ontology_dependency_failures_use_safe_internal_and_unavailable_error_envelopes() {
    let (status, _headers, body) =
        response_json(ApiError::from_ontology(OntologyStoreError::CorruptData).into_response())
            .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_error(&body, "internal_error");

    let connection_error = match OntologyStore::connect("not a PostgreSQL URL").await {
        Err(error) => error,
        Ok(_) => panic!("malformed PostgreSQL URL must not connect"),
    };
    let (status, _headers, body) =
        response_json(ApiError::from_ontology(connection_error).into_response()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_error(&body, "ontology_store_unavailable");
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn ontology_router_reports_dependency_aware_readiness() {
    let fixture = OntologyFixture::new().await;

    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get("/health/ready")
                    .body(Body::empty())
                    .expect("readiness request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok", "realtime": "degraded" }));
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn ontology_router_crud_list_neighborhood_and_round_trip_preserve_exact_wire_contract() {
    let fixture = OntologyFixture::new().await;
    let created = fixture
        .create(&unique_name("zzzzapi_crud"), Some("Created description"))
        .await;

    assert_eq!(
        created.body,
        json!({
            "id": created.id,
            "name": created.body["name"].clone(),
            "display_name": "HTTP verification Ontology",
            "description": "Created description",
            "object_types": [],
            "link_types": [],
            "canvas": {"positions": []},
        })
    );

    let (status, headers, body) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{}", created.id))
                    .body(Body::empty())
                    .expect("read request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, ETAG), created.etag);
    assert_eq!(body, created.body);

    let (document, ids) = candidate(&created.id, &string_field(&created.body, "name"));
    let (status, headers, body) = response_bytes(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", created.id),
                document.clone(),
                &created.etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 replace response must not have a body");
    let first_put_etag = header_value(&headers, ETAG);
    assert_ne!(first_put_etag, created.etag);

    let (status, headers, round_tripped) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{}", created.id))
                    .body(Body::empty())
                    .expect("read request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, ETAG), first_put_etag);
    assert_eq!(round_tripped, document);
    assert_eq!(
        round_tripped["object_types"]
            .as_array()
            .expect("object types are an array")[0]["id"],
        ids.person
    );
    assert_eq!(
        round_tripped["canvas"]["positions"]
            .as_array()
            .expect("canvas positions are an array")[0]["object_type_id"],
        ids.company
    );

    let (status, headers, body) = response_json(
        fixture
            .request(
                Request::get(format!(
                    "/v1/ontologies/{}/object-types/{}/neighborhood",
                    created.id, ids.person
                ))
                .body(Body::empty())
                .expect("neighborhood request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers.get(ETAG).is_none(),
        "neighborhood must not have an ETag"
    );
    assert_eq!(body, expected_neighborhood(&document, &ids.person));
    assert!(body.get("id").is_none(), "neighborhood is not a Candidate");
    assert!(
        body.get("name").is_none(),
        "neighborhood omits root metadata"
    );

    let list_name = unique_name("zzzzapi_list");
    let list_created = fixture.create(&list_name, None).await;
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get("/v1/ontologies?page=1&per_page=100&sort=-name")
                    .body(Body::empty())
                    .expect("list request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.as_object()
            .expect("list response is an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["data", "pagination"]
    );
    assert_eq!(body["pagination"]["page"], 1);
    assert_eq!(body["pagination"]["per_page"], 100);
    assert!(body["pagination"]["total"].as_i64().is_some());
    let summary = body["data"]
        .as_array()
        .expect("list data is an array")
        .iter()
        .find(|summary| summary["id"] == list_created.id)
        .expect("new summary is in the first descending-name page");
    assert_eq!(summary["name"], list_name);
    assert_eq!(summary["display_name"], "HTTP verification Ontology");
    assert!(summary.get("description").is_none());
    assert!(summary["created_at"].as_str().is_some());
    assert!(summary["updated_at"].as_str().is_some());
    assert!(summary.get("object_types").is_none());

    let (status, _headers, default_page) = response_json(
        fixture
            .request(
                Request::get("/v1/ontologies")
                    .body(Body::empty())
                    .expect("default list request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(default_page["pagination"]["page"], 1);
    assert_eq!(default_page["pagination"]["per_page"], 20);

    let (status, _headers, out_of_range_page) = response_json(
        fixture
            .request(
                Request::get("/v1/ontologies?page=999999&per_page=1&sort=name")
                    .body(Body::empty())
                    .expect("out-of-range list request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(out_of_range_page["data"], json!([]));
    assert!(out_of_range_page["pagination"]["total"].as_i64().is_some());

    let (status, headers, body) = response_bytes(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", created.id),
                round_tripped.clone(),
                &first_put_etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "same-document PUT must remain bodyless");
    let second_put_etag = header_value(&headers, ETAG);
    assert_ne!(second_put_etag, first_put_etag);

    let (status, _headers, reread) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{}", created.id))
                    .body(Body::empty())
                    .expect("read request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reread, document,
        "GET payload can be used as a lossless PUT DTO"
    );

    let (status, headers, body) = response_bytes(
        fixture
            .request(empty_request_with_etag(
                Method::DELETE,
                format!("/v1/ontologies/{}", created.id),
                &second_put_etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 delete response must not have a body");
    assert!(
        headers.get(ETAG).is_none(),
        "delete does not issue a new ETag"
    );

    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{}", created.id))
                    .body(Body::empty())
                    .expect("read request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&body, "ontology_not_found");
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn ontology_router_rejects_strict_payloads_invalid_pagination_and_oversized_bodies() {
    let fixture = OntologyFixture::new().await;
    let base_name = unique_name("zzzzapi_strict");
    for payload in [
        json!({
            "name": base_name,
            "display_name": "Strict input",
            "unexpected": true,
        }),
        json!({
            "name": unique_name("zzzzapi_null"),
            "display_name": "Strict input",
            "description": null,
        }),
        json!({
            "name": unique_name("zzzzapi_missing"),
        }),
    ] {
        let (status, _headers, body) = response_json(
            fixture
                .request(json_request(Method::POST, "/v1/ontologies", payload))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error(&body, "invalid_request");
    }

    let (status, _headers, body) = response_json(
        fixture
            .request(json_request(
                Method::POST,
                "/v1/ontologies",
                json!({
                    "name": "invalid-name",
                    "display_name": "Structurally valid but semantically invalid",
                }),
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_ontology_schema");
    assert_eq!(
        body["error"]["violations"],
        json!([
            {
            "code": "invalid_ontology_name",
            "path": "/name",
            "message": "name must match the required pattern",
            }
        ])
    );

    for query in [
        "?page=0",
        "?per_page=101",
        "?sort=unsupported",
        "?unknown=value",
    ] {
        let (status, _headers, body) = response_json(
            fixture
                .request(
                    Request::get(format!("/v1/ontologies{query}"))
                        .body(Body::empty())
                        .expect("list request builds"),
                )
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "query {query}");
        assert_error(&body, "invalid_request");
    }

    let (status, _headers, body) = response_json(
        fixture
            .request(json_request(
                Method::POST,
                "/v1/ontologies",
                json!({
                    "name": unique_name("zzzzapi_route_limit"),
                    "display_name": "Route-specific body limit",
                    "description": "x".repeat(70 * 1024),
                }),
            ))
            .await,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Ontology routes must override the agent API's 64 KiB default up to 2 MiB"
    );
    assert_eq!(body["error"]["code"], "invalid_ontology_schema");

    let created = fixture
        .create(&unique_name("zzzzapi_put_strict"), None)
        .await;
    let (document, _) = candidate(&created.id, &string_field(&created.body, "name"));
    for invalid in [
        {
            let mut payload = document.clone();
            payload["unexpected"] = json!(true);
            payload
        },
        {
            let mut payload = document.clone();
            payload["object_types"][0]["description"] = Value::Null;
            payload
        },
        {
            let mut payload = document.clone();
            payload["object_types"][0]["id"] = json!("550e8400-e29b-41d4-a716-446655440000");
            payload
        },
        {
            let mut payload = document.clone();
            payload["object_types"][0]["properties"][0]["value_type"] = json!("object");
            payload
        },
    ] {
        let (status, _headers, body) = response_json(
            fixture
                .request(json_request_with_etag(
                    Method::PUT,
                    format!("/v1/ontologies/{}", created.id),
                    invalid,
                    &created.etag,
                ))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_error(&body, "invalid_request");
    }

    let oversized = vec![b'x'; 2 * 1024 * 1024 + 1];
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::post("/v1/ontologies")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(oversized))
                    .expect("oversized Ontology request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_error(&body, "ontology_payload_too_large");

    let oversized_put = vec![b'x'; 2 * 1024 * 1024 + 1];
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/v1/ontologies/{}", created.id))
                    .header(CONTENT_TYPE, "application/json")
                    .header(IF_MATCH, &created.etag)
                    .body(Body::from(oversized_put))
                    .expect("oversized Ontology replacement builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_error(&body, "ontology_payload_too_large");

    let unrelated_oversized = vec![b'x'; 64 * 1024 + 1];
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::post("/v1/agent-runtimes")
                    .header(CONTENT_TYPE, "application/json")
                    .header("Idempotency-Key", Uuid::now_v7().to_string())
                    .body(Body::from(unrelated_oversized))
                    .expect("oversized unrelated request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_error(&body, "request_too_large");
}

#[tokio::test]
#[ignore = "requires the stratum-ontology PostgreSQL container"]
async fn ontology_router_maps_statuses_and_preserves_data_for_stale_and_invalid_replacements() {
    let fixture = OntologyFixture::new().await;
    let first = fixture.create(&unique_name("zzzzapi_first"), None).await;
    let second = fixture.create(&unique_name("zzzzapi_second"), None).await;

    let (status, _headers, body) = response_json(
        fixture
            .request(json_request(
                Method::POST,
                "/v1/ontologies",
                json!({
                    "name": first.body["name"].clone(),
                    "display_name": "Duplicate name",
                }),
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error(&body, "ontology_name_conflict");

    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get("/v1/ontologies/not-a-uuid")
                    .body(Body::empty())
                    .expect("invalid path request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error(&body, "invalid_request");

    let missing_id = Uuid::now_v7().to_string();
    let missing_etag = format!("\"ontology:{missing_id}:1\"");
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{missing_id}"))
                    .body(Body::empty())
                    .expect("missing read request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&body, "ontology_not_found");
    let (status, _headers, body) = response_json(
        fixture
            .request(empty_request_with_etag(
                Method::DELETE,
                format!("/v1/ontologies/{missing_id}"),
                &missing_etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&body, "ontology_not_found");
    let (missing_document, _) = candidate(&missing_id, &unique_name("zzzzapi_missing"));
    let (status, _headers, body) = response_json(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{missing_id}"),
                missing_document,
                &missing_etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&body, "ontology_not_found");

    let (first_document, first_ids) = candidate(&first.id, &string_field(&first.body, "name"));
    let (status, headers, body) = response_bytes(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", first.id),
                first_document.clone(),
                &first.etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
    let current_first_etag = header_value(&headers, ETAG);

    let (status, _headers, body) = response_json(
        fixture
            .request(json_request(
                Method::PUT,
                format!("/v1/ontologies/{}", first.id),
                first_document.clone(),
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_error(&body, "ontology_precondition_required");
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/v1/ontologies/{}", first.id))
                    .body(Body::empty())
                    .expect("unconditional delete builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_error(&body, "ontology_precondition_required");

    for invalid_syntax in ["W/\"not-strong\"", "*", "ontology:not-a-quoted-entity-tag"] {
        let (status, _headers, body) = response_json(
            fixture
                .request(json_request_with_etag(
                    Method::PUT,
                    format!("/v1/ontologies/{}", first.id),
                    first_document.clone(),
                    invalid_syntax,
                ))
                .await,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "invalid If-Match {invalid_syntax}"
        );
        assert_error(&body, "invalid_request");
    }
    let mut repeated_if_match = json_request_with_etag(
        Method::PUT,
        format!("/v1/ontologies/{}", first.id),
        first_document.clone(),
        &current_first_etag,
    );
    repeated_if_match.headers_mut().append(
        IF_MATCH,
        current_first_etag
            .parse()
            .expect("canonical ETag is a valid HTTP header"),
    );
    let (status, _headers, body) = response_json(fixture.request(repeated_if_match).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error(&body, "invalid_request");

    let mut mismatched_document_id = first_document.clone();
    mismatched_document_id["id"] = json!(Uuid::now_v7().to_string());
    let (status, _headers, body) = response_json(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", first.id),
                mismatched_document_id,
                &current_first_etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error(&body, "invalid_request");

    let mut changed_but_stale = first_document.clone();
    changed_but_stale["display_name"] = json!("Must not persist");
    let (status, _headers, body) = response_json(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", first.id),
                changed_but_stale,
                &second.etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_error(&body, "ontology_precondition_failed");

    let revision = current_first_etag
        .trim_matches('"')
        .rsplit(':')
        .next()
        .expect("canonical ETag has a revision");
    for noncanonical_but_parseable in [
        format!("\"ontology:{}:0{revision}\"", first.id),
        format!("\"ontology:{}:+{revision}\"", first.id),
        format!("\"ontology:{}:{revision}\"", first.id.to_uppercase()),
        "\"unrelated-strong-opaque-tag\"".to_owned(),
    ] {
        let (status, _headers, body) = response_json(
            fixture
                .request(json_request_with_etag(
                    Method::PUT,
                    format!("/v1/ontologies/{}", first.id),
                    first_document.clone(),
                    &noncanonical_but_parseable,
                ))
                .await,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::PRECONDITION_FAILED,
            "noncanonical strong ETag {noncanonical_but_parseable} is stale"
        );
        assert_error(&body, "ontology_precondition_failed");
    }

    let mut obs_text_if_match = json_request(
        Method::PUT,
        format!("/v1/ontologies/{}", first.id),
        first_document.clone(),
    );
    obs_text_if_match.headers_mut().insert(
        IF_MATCH,
        HeaderValue::from_bytes(b"\"opaque-\x80-tag\"")
            .expect("obs-text entity-tag is a valid header value"),
    );
    let (status, _headers, body) = response_json(fixture.request(obs_text_if_match).await).await;
    assert_eq!(
        status,
        StatusCode::PRECONDITION_FAILED,
        "a syntactically valid obs-text entity-tag is stale"
    );
    assert_error(&body, "ontology_precondition_failed");

    let (status, headers, body) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{}", first.id))
                    .body(Body::empty())
                    .expect("verification read builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, ETAG), current_first_etag);
    assert_eq!(body, first_document);

    let (second_name_conflict_document, _) =
        candidate(&second.id, &string_field(&first.body, "name"));
    let (status, _headers, body) = response_json(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", second.id),
                second_name_conflict_document,
                &second.etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error(&body, "ontology_name_conflict");

    let (status, _headers, body) = response_json(
        fixture
            .request(empty_request_with_etag(
                Method::DELETE,
                format!("/v1/ontologies/{}", first.id),
                &second.etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_error(&body, "ontology_precondition_failed");

    let mut invalid = first_document.clone();
    invalid["name"] = json!("invalid-name");
    invalid["object_types"][0]["display_name"] = json!("");
    invalid["object_types"][1]["properties"][0]["name"] = json!("invalid-name");
    invalid["link_types"][0]["source_object_type_id"] = json!(Uuid::now_v7().to_string());
    invalid["canvas"]["positions"]
        .as_array_mut()
        .expect("positions are an array")
        .push(json!({
            "object_type_id": first_ids.person,
            "x": 10.0,
            "y": 20.0,
        }));
    let (status, _headers, invalid_body) = response_json(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", first.id),
                invalid,
                &current_first_etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let violations = invalid_body["error"]["violations"]
        .as_array()
        .expect("422 response contains violations");
    assert_eq!(invalid_body["error"]["code"], "invalid_ontology_schema");
    assert!(violations.len() >= 5, "independent violations are retained");
    assert!(violations.windows(2).all(|pair| {
        let left = (
            pair[0]["path"].as_str().expect("violation has path"),
            pair[0]["code"].as_str().expect("violation has code"),
        );
        let right = (
            pair[1]["path"].as_str().expect("violation has path"),
            pair[1]["code"].as_str().expect("violation has code"),
        );
        left <= right
    }));
    for expected in [
        (
            "duplicate_canvas_position",
            "/canvas/positions/2/object_type_id",
        ),
        (
            "unknown_link_source_object_type",
            "/link_types/0/source_object_type_id",
        ),
        ("invalid_ontology_name", "/name"),
        ("invalid_display_name", "/object_types/0/display_name"),
        ("invalid_property_name", "/object_types/1/properties/0/name"),
    ] {
        assert!(violations.iter().any(|violation| {
            violation["code"] == expected.0 && violation["path"] == expected.1
        }));
    }

    let (status, headers, body) = response_json(
        fixture
            .request(
                Request::get(format!("/v1/ontologies/{}", first.id))
                    .body(Body::empty())
                    .expect("post-validation read builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(header_value(&headers, ETAG), current_first_etag);
    assert_eq!(body, first_document);

    let (mut second_document, _) = candidate(&second.id, &string_field(&second.body, "name"));
    second_document["object_types"][0]["id"] = first_document["object_types"][0]["id"].clone();
    second_document["link_types"][0]["target_object_type_id"] =
        second_document["object_types"][0]["id"].clone();
    second_document["link_types"][1]["source_object_type_id"] =
        second_document["object_types"][0]["id"].clone();
    second_document["link_types"][1]["target_object_type_id"] =
        second_document["object_types"][0]["id"].clone();
    second_document["canvas"]["positions"][1]["object_type_id"] =
        second_document["object_types"][0]["id"].clone();
    let (status, _headers, body) = response_json(
        fixture
            .request(json_request_with_etag(
                Method::PUT,
                format!("/v1/ontologies/{}", second.id),
                second_document,
                &second.etag,
            ))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error(&body, "ontology_entity_id_conflict");

    let missing_object_type_id = Uuid::now_v7();
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get(format!(
                    "/v1/ontologies/{}/object-types/{missing_object_type_id}/neighborhood?depth=1",
                    first.id
                ))
                .body(Body::empty())
                .expect("missing origin request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&body, "object_type_not_found");
    let (status, _headers, body) = response_json(
        fixture
            .request(
                Request::get(format!(
                    "/v1/ontologies/{}/object-types/{}/neighborhood?depth=6",
                    first.id, first_ids.person
                ))
                .body(Body::empty())
                .expect("invalid depth request builds"),
            )
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_error(&body, "invalid_request");
}
