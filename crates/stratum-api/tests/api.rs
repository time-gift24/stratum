//! HTTP-level integration tests against the real Postgres (+ NATS) compose
//! stack. Every test is `#[ignore]`d for the default test run; the crate
//! `Makefile` brings the stack up and runs them with `--test-threads=1`.

mod common;

use axum::http::StatusCode;
use common::{
    Fixture, MockProvider, Script, TEST_MODEL, read_sse_events, read_sse_until, restarted, uuid_v7,
    view, wait_until,
};
use serde_json::{Value, json};
use stratum_core::{
    AgentId, AgentVersionId, ExtensionSetVersionId, ModelConfig, ModelId, SessionId,
    SkillSetVersionId, TurnId, TurnRuntimeSnapshot,
};
use stratum_postgres::{BeginTurn, PostgresBackend};

const TEMPLATE: &str = r#"prompt = "You are a helpful test agent."
"#;

const TOOL_TEMPLATE: &str = r#"tools = ["echo"]
prompt = "You are a helpful test agent with tools."
"#;

fn tool_call_events(call_id: &str, arguments: &str) -> Script {
    Script::Events(vec![
        stratum_llm::ChatStreamEvent::ToolCallDelta(stratum_core::ToolCallDelta {
            index: 0,
            call_id: Some(stratum_core::CallId::from(call_id)),
            name: Some("echo".to_owned()),
            arguments_delta: arguments.to_owned(),
        }),
        stratum_llm::ChatStreamEvent::Finished {
            finish_reason: stratum_llm::FinishReason::ToolCalls,
            usage: Some(stratum_core::TokenUsage {
                input_tokens: 4,
                output_tokens: 1,
                total_tokens: 5,
            }),
        },
    ])
}

fn model_config() -> Value {
    json!({ "model": TEST_MODEL, "parameters": {} })
}

async fn create_agent(fixture: &Fixture, key: &str, name: &str) -> (StatusCode, Value) {
    fixture
        .json(
            "POST",
            "/v1/agents",
            Some(json!({ "agent_name": name })),
            Some(key),
        )
        .await
}

async fn send_message(
    fixture: &Fixture,
    agent_id: &str,
    text: &str,
    expected: Value,
) -> (StatusCode, Value) {
    fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/messages"),
            Some(json!({ "text": text, "expected_current_turn_id": expected })),
            None,
        )
        .await
}

async fn wait_for_status(fixture: &Fixture, agent_id: &str, expected: &str) -> Value {
    wait_until(10, || async {
        let latest = view(fixture, agent_id).await;
        (latest["status"] == expected).then_some(latest)
    })
    .await
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn create_agent_idempotency_matrix() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE), ("agent-b", TOOL_TEMPLATE)], vec![]).await;

    // Missing or malformed key.
    let (status, body) = fixture
        .json(
            "POST",
            "/v1/agents",
            Some(json!({ "agent_name": "agent-a" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, _) = create_agent(&fixture, "not-a-uuid", "agent-a").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Unknown template.
    let (status, body) = create_agent(&fixture, &uuid_v7(), "missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "template_not_found");

    // Pure create: 201, Location, idle, no turn.
    let key = uuid_v7();
    let (status, created) = create_agent(&fixture, &key, "agent-a").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    assert_eq!(created["agent_name"], "agent-a");
    let agent_view = view(&fixture, &agent_id).await;
    assert_eq!(agent_view["status"], "idle");
    assert_eq!(agent_view["snapshot_event_seq"], "0");
    assert_eq!(agent_view["telemetry_floor_event_seq"], "0");
    assert!(agent_view["session_id"].is_null());
    assert!(agent_view["current_turn_id"].is_null());
    assert_eq!(agent_view["resume_required"], false);

    // Identical replay: same key and request, even after the template changed.
    fixture.write_template("agent-a", "prompt = \"A changed prompt.\"\n");
    let (status, replay) = create_agent(&fixture, &key, "agent-a").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(replay, created, "replay returns the identical body");

    // Same key with a different request conflicts.
    let (status, body) = create_agent(&fixture, &key, "agent-b").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "idempotency_key_conflict");
    let (status, body) = fixture
        .json(
            "POST",
            "/v1/agents",
            Some(json!({ "agent_name": "agent-a", "model_config": model_config() })),
            Some(&key),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "idempotency_key_conflict");

    // Unknown body fields (including credential-shaped ones) are rejected.
    let (status, body) = fixture
        .json(
            "POST",
            "/v1/agents",
            Some(json!({ "agent_name": "agent-a", "api_key": "sk-secret" })),
            Some(&uuid_v7()),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");

    // Invalid template and model preflight failures.
    fixture.write_template("broken", "prompt = 42\n");
    let (status, body) = create_agent(&fixture, &uuid_v7(), "broken").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_agent_template");
    fixture.write_template(
        "bad-model",
        "model = \"openai:not-configured\"\nprompt = \"x\"\n",
    );
    let (status, body) = create_agent(&fixture, &uuid_v7(), "bad-model").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "model_not_configured");
    let (status, body) = fixture
        .json(
            "POST",
            "/v1/agents",
            Some(
                json!({ "agent_name": "agent-a", "model_config": { "model": TEST_MODEL, "parameters": { "temperature": 1 } } }),
            ),
            Some(&uuid_v7()),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_model_parameters");

    // A failed create never consumes the key.
    let retry_key = uuid_v7();
    let (status, _) = create_agent(&fixture, &retry_key, "broken").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    fixture.write_template("broken", TEMPLATE);
    let (status, created) = create_agent(&fixture, &retry_key, "broken").await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn template_catalog_is_all_or_nothing_and_safe() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![]).await;

    let (status, body) = fixture.json("GET", "/v1/agent-templates", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let templates = body["templates"].as_array().expect("templates list");
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0]["agent_name"], "agent-a");
    assert_eq!(templates[0]["model_config"]["model"], TEST_MODEL);
    let raw = body.to_string();
    assert!(
        !raw.contains("helpful test agent"),
        "prompts never leak: {raw}"
    );
    assert!(!raw.contains(&fixture.root.to_string_lossy().to_string()));

    // One invalid template fails the whole catalog.
    fixture.write_template("broken", "prompt = 1\n");
    let (status, body) = fixture.json("GET", "/v1/agent-templates", None, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_agent_template");
    fixture.remove_template("broken");
    let (status, _) = fixture.json("GET", "/v1/agent-templates", None, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn models_endpoint_lists_configured_models_with_schemas() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![]).await;
    let (status, body) = fixture.json("GET", "/v1/models", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let models = body["models"].as_array().expect("models list");
    assert!(
        models
            .iter()
            .any(|model| model["model"] == TEST_MODEL && model["parameters_schema"].is_object())
    );
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn message_admission_cas_and_session_rules() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], MockProvider::text("first answer")).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();

    // The expected key is required; empty text is rejected.
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/messages"),
            Some(json!({ "text": "hello" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, _) = send_message(&fixture, &agent_id, "   ", Value::Null).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = send_message(&fixture, &agent_id, "hello", json!(uuid_v7())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");

    // First turn: admitted, session generated server-side.
    let (status, accepted) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let turn_one = accepted["turn_id"].as_str().expect("turn id").to_owned();
    assert!(accepted["session_id"].is_string());
    let finished = wait_for_status(&fixture, &agent_id, "finished").await;
    assert_eq!(finished["current_turn_id"], turn_one);
    assert_eq!(finished["latest_usage"]["total_tokens"], 5);

    // A lost-response retry with the old expectation is stale, even though
    // the first turn already terminated.
    let (status, body) = send_message(&fixture, &agent_id, "hello again", Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");

    // A different explicit session is rejected; the bound one is reused.
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/messages"),
            Some(
                json!({ "text": "hi", "expected_current_turn_id": turn_one, "session_id": uuid_v7() }),
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "session_mismatch");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn busy_hosted_and_unhosted_running_turns_reject_new_messages() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![Script::Pending]).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let (status, accepted) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();

    // Hosted running turn: agent_busy.
    let (status, body) = send_message(&fixture, &agent_id, "again", json!(turn_id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "agent_busy");

    // Restarted host (empty registry): resume_required, and the view carries
    // the advisory.
    let restarted = restarted(&fixture, vec![]).await;
    let unhosted = view(&restarted, &agent_id).await;
    assert_eq!(unhosted["status"], "running");
    assert_eq!(unhosted["resume_required"], true);
    let (status, body) = send_message(&restarted, &agent_id, "again", json!(turn_id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "resume_required");

    // Clean up the running turn through the original host.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/cancel"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&fixture, &agent_id, "cancelled").await;
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn session_allows_only_one_running_agent() {
    let fixture = Fixture::new(
        &[("agent-a", TEMPLATE), ("agent-b", TEMPLATE)],
        vec![Script::Pending],
    )
    .await;
    let session = uuid_v7();
    let (_, first) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let (_, second) = create_agent(&fixture, &uuid_v7(), "agent-b").await;
    let first_id = first["agent_id"].as_str().expect("agent id").to_owned();
    let second_id = second["agent_id"].as_str().expect("agent id").to_owned();

    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{first_id}/messages"),
            Some(json!({ "text": "hi", "expected_current_turn_id": null, "session_id": session })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{second_id}/messages"),
            Some(json!({ "text": "hi", "expected_current_turn_id": null, "session_id": session })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "session_busy");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn started_only_turn_is_reconciled_by_resume() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], MockProvider::text("recovered")).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();

    // Simulate the crash window directly through the store: a committed
    // LoopStarted without the first user message.
    let pg = PostgresBackend::connect(&common::pg_url())
        .await
        .expect("postgres connects");
    let agent_uuid = agent_id.parse::<uuid::Uuid>().expect("agent uuid");
    let turn_id = TurnId::new();
    let session_id = SessionId::new();
    let snapshot = TurnRuntimeSnapshot::new(
        AgentVersionId::new(),
        ModelConfig::new(
            ModelId::new("openai", "test-model").expect("model id is valid"),
            serde_json::Map::new(),
        ),
        "ab".repeat(32).parse().expect("fingerprint is valid"),
        SkillSetVersionId::new(),
        ExtensionSetVersionId::new(),
        Vec::new(),
    );
    pg.begin_turn(BeginTurn {
        agent_id: AgentId::from(agent_uuid),
        expected_current_turn_id: None,
        turn_id,
        session_id,
        snapshot,
    })
    .await
    .expect("started-only turn commits");

    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/resume"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "turn_preamble_incomplete");

    let failed = view(&fixture, &agent_id).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["current_turn_id"], json!(turn_id));

    // The next turn starts from the original default model and succeeds.
    let (status, accepted) = send_message(&fixture, &agent_id, "hello", json!(turn_id)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    wait_for_status(&fixture, &agent_id, "finished").await;
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn model_override_replaces_the_default_after_the_first_message() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], MockProvider::text("answer")).await;
    let override_model = json!({ "model": TEST_MODEL, "parameters": {} });
    let (_, created) = fixture
        .json(
            "POST",
            "/v1/agents",
            Some(json!({ "agent_name": "agent-a", "model_config": override_model })),
            Some(&uuid_v7()),
        )
        .await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    assert_eq!(created["model_config"], override_model);

    let (status, accepted) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/messages"),
            Some(
                json!({ "text": "hi", "expected_current_turn_id": null, "model_config": override_model }),
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    wait_for_status(&fixture, &agent_id, "finished").await;
    let after = view(&fixture, &agent_id).await;
    assert_eq!(after["model_config"], override_model);
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn approval_request_resolve_consume_lifecycle() {
    let fixture = Fixture::new(
        &[("agent-a", TOOL_TEMPLATE)],
        vec![
            tool_call_events("call-1", r#"{"text":"hello"}"#),
            tool_call_events("call-2", r#"{"text":"again"}"#),
            Script::Events(vec![
                stratum_llm::ChatStreamEvent::TextDelta {
                    delta: "tool done".to_owned(),
                },
                stratum_llm::ChatStreamEvent::Finished {
                    finish_reason: stratum_llm::FinishReason::Stop,
                    usage: Some(stratum_core::TokenUsage {
                        input_tokens: 6,
                        output_tokens: 2,
                        total_tokens: 8,
                    }),
                },
            ]),
        ],
    )
    .await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let (status, accepted) = send_message(&fixture, &agent_id, "run echo", Value::Null).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();

    // The approval request surfaces in the view with safe fields only.
    let pending = wait_until(10, || async {
        let latest = view(&fixture, &agent_id).await;
        (!latest["pending_approvals"]
            .as_array()
            .expect("array")
            .is_empty())
        .then_some(latest)
    })
    .await;
    let approval = &pending["pending_approvals"][0];
    let approval_id = approval["approval_id"]
        .as_str()
        .expect("approval id")
        .to_owned();
    assert_eq!(approval["call_id"], "call-1");
    assert_eq!(approval["tool_name"], "echo");
    assert_eq!(approval["arguments"], json!({ "text": "hello" }));
    assert!(approval.get("hook_invocation_id").is_none());
    assert_eq!(pending["resume_required"], false);

    // Identity fences.
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": uuid_v7(), "decision": "approve" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{}", uuid_v7()),
            Some(json!({ "turn_id": turn_id, "decision": "approve" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "approval_not_found");

    // First resolve commits; the turn consumes the decision and parks on
    // the second approval.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": turn_id, "decision": "approve" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // While the turn is still running: the same decision replays 204 and the
    // opposite decision conflicts.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": turn_id, "decision": "approve" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": turn_id, "decision": "reject" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "approval_already_resolved");

    // The second approval appears; rejecting it lets the loop finish.
    let second_pending = wait_until(10, || async {
        let latest = view(&fixture, &agent_id).await;
        let approvals = latest["pending_approvals"].as_array().expect("array");
        approvals
            .iter()
            .find(|entry| entry["approval_id"] != json!(approval_id))
            .map(|entry| entry["approval_id"].as_str().expect("id").to_owned())
    })
    .await;
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{second_pending}"),
            Some(json!({ "turn_id": turn_id, "decision": "reject" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let finished = wait_for_status(&fixture, &agent_id, "finished").await;
    assert!(
        finished["pending_approvals"]
            .as_array()
            .expect("array")
            .is_empty()
    );
    assert_eq!(finished["latest_usage"]["total_tokens"], 8);
    assert_eq!(fixture.provider.calls(), 3);

    // Terminal invalidates every further decision attempt.
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": turn_id, "decision": "approve" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "approval_invalidated");

    // History shows the tool result as a role=tool message.
    let barrier = finished["snapshot_event_seq"].as_str().expect("barrier");
    let (status, history) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let roles: Vec<&str> = history["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["event"]["data"]["message"]["role"].as_str())
        .collect();
    assert!(roles.contains(&"tool"), "tool result in history: {roles:?}");
    assert!(roles.contains(&"assistant"));
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn reject_maps_to_a_blocked_tool_result_and_the_turn_continues() {
    let fixture = Fixture::new(
        &[("agent-a", TOOL_TEMPLATE)],
        MockProvider::tool_call_then_text("call-9", r#"{"text":"x"}"#, "continued after block"),
    )
    .await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let (_, accepted) = send_message(&fixture, &agent_id, "run echo", Value::Null).await;
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();

    let pending = wait_until(10, || async {
        let latest = view(&fixture, &agent_id).await;
        (!latest["pending_approvals"]
            .as_array()
            .expect("array")
            .is_empty())
        .then_some(latest)
    })
    .await;
    let approval_id = pending["pending_approvals"][0]["approval_id"]
        .as_str()
        .expect("approval id")
        .to_owned();
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": turn_id, "decision": "reject" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The blocked call becomes a tool result and the loop continues.
    wait_for_status(&fixture, &agent_id, "finished").await;
    assert_eq!(fixture.provider.calls(), 2);
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn cancel_races_are_stable() {
    let fixture = Fixture::new(
        &[("agent-a", TEMPLATE)],
        vec![Script::Pending, Script::Pending],
    )
    .await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let (_, accepted) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();

    // Stale turn and unknown agent.
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/cancel"),
            Some(json!({ "turn_id": uuid_v7() })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");

    // Hosted running turn: 202, then the durable terminal arrives.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/cancel"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&fixture, &agent_id, "cancelled").await;

    // Idempotent repeat and the not-running sibling case.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/cancel"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Unhosted running turn: turn_not_hosted.
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let second = created["agent_id"].as_str().expect("agent id").to_owned();
    let (_, accepted) = send_message(&fixture, &second, "hello", Value::Null).await;
    let second_turn = accepted["turn_id"].as_str().expect("turn id").to_owned();
    let restarted = restarted(&fixture, vec![]).await;
    let (status, body) = restarted
        .json(
            "POST",
            &format!("/v1/agents/{second}/cancel"),
            Some(json!({ "turn_id": second_turn })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "turn_not_hosted");

    // Cleanup, then the idempotent repeat of the same cancelled turn.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{second}/cancel"),
            Some(json!({ "turn_id": second_turn })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&fixture, &second, "cancelled").await;
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{second}/cancel"),
            Some(json!({ "turn_id": second_turn })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn resume_after_crash_reuses_approval_and_journal_without_reasking() {
    let fixture = Fixture::new(
        &[("agent-a", TOOL_TEMPLATE)],
        vec![Script::Events(vec![
            stratum_llm::ChatStreamEvent::ToolCallDelta(stratum_core::ToolCallDelta {
                index: 0,
                call_id: Some(stratum_core::CallId::from("call-1")),
                name: Some("echo".to_owned()),
                arguments_delta: r#"{"text":"hello"}"#.to_owned(),
            }),
            stratum_llm::ChatStreamEvent::Finished {
                finish_reason: stratum_llm::FinishReason::ToolCalls,
                usage: None,
            },
        ])],
    )
    .await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let (_, accepted) = send_message(&fixture, &agent_id, "run echo", Value::Null).await;
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();

    let pending = wait_until(10, || async {
        let latest = view(&fixture, &agent_id).await;
        (!latest["pending_approvals"]
            .as_array()
            .expect("array")
            .is_empty())
        .then_some(latest)
    })
    .await;
    let approval_id = pending["pending_approvals"][0]["approval_id"]
        .as_str()
        .expect("approval id")
        .to_owned();

    // Crash before the decision: a restarted host takes over the exact turn.
    let restarted = restarted(&fixture, MockProvider::text("resumed answer")).await;
    let (status, accepted) = restarted
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/resume"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["turn_id"], json!(turn_id));

    // Already hosted: a second resume is a 204.
    let (status, _) = restarted
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/resume"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The provider was not re-asked and no second approval request exists.
    assert_eq!(restarted.provider.calls(), 0);
    let pg = PostgresBackend::connect(&common::pg_url())
        .await
        .expect("postgres connects");
    let agent_uuid = agent_id.parse::<uuid::Uuid>().expect("agent uuid");
    let agent_view = view(&restarted, &agent_id).await;
    let barrier: u64 = agent_view["snapshot_event_seq"]
        .as_str()
        .expect("barrier")
        .parse()
        .expect("barrier parses");
    let rows = pg
        .read_events_range(AgentId::from(agent_uuid), 0, barrier)
        .await
        .expect("events read");
    let requested = rows
        .iter()
        .filter(|row| {
            matches!(
                row.event,
                stratum_core::DurableAgentEvent::ToolApprovalRequested { .. }
            )
        })
        .count();
    assert_eq!(requested, 1, "the request is reused across the crash");

    // The same approval stays pending and resolves the resumed turn.
    let still_pending = view(&restarted, &agent_id).await;
    assert_eq!(
        still_pending["pending_approvals"][0]["approval_id"],
        json!(approval_id)
    );
    let (status, _) = restarted
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/approvals/{approval_id}"),
            Some(json!({ "turn_id": turn_id, "decision": "approve" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    wait_for_status(&restarted, &agent_id, "finished").await;
    assert_eq!(restarted.provider.calls(), 1);
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn resume_rejects_stale_and_not_running_turns() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], MockProvider::text("done")).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();

    // Idle agent: the turn is not the current one.
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/resume"),
            Some(json!({ "turn_id": uuid_v7() })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");

    // Terminal turn: not running.
    let (_, accepted) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();
    wait_for_status(&fixture, &agent_id, "finished").await;
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/resume"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "turn_not_running");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn history_paginates_and_exposes_compaction_markers_safely() {
    let fixture = Fixture::new(
        &[("agent-a", TOOL_TEMPLATE)],
        vec![
            Script::Events(vec![
                stratum_llm::ChatStreamEvent::TextDelta {
                    delta: "one".to_owned(),
                },
                stratum_llm::ChatStreamEvent::Finished {
                    finish_reason: stratum_llm::FinishReason::Stop,
                    usage: None,
                },
            ]),
            tool_call_events("call-1", r#"{"text":"two"}"#),
            Script::Pending,
        ],
    )
    .await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();

    let (_, first) = send_message(&fixture, &agent_id, "first", Value::Null).await;
    let first_turn = first["turn_id"].as_str().expect("turn id").to_owned();
    wait_for_status(&fixture, &agent_id, "finished").await;
    let (_, second) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/messages"),
            Some(json!({ "text": "second", "expected_current_turn_id": first_turn })),
            None,
        )
        .await;
    let second_turn = second["turn_id"].as_str().expect("turn id").to_owned();

    // The second turn parks on the echo approval, so a compaction can be
    // committed into the running turn directly through the store.
    let _pending = wait_until(10, || async {
        let latest = view(&fixture, &agent_id).await;
        (!latest["pending_approvals"]
            .as_array()
            .expect("array")
            .is_empty())
        .then_some(latest)
    })
    .await;
    let pg = PostgresBackend::connect(&common::pg_url())
        .await
        .expect("postgres connects");
    let agent_uuid = agent_id.parse::<uuid::Uuid>().expect("agent uuid");
    let agent_state = pg
        .read_agent_state(AgentId::from(agent_uuid))
        .await
        .expect("state reads");
    pg.append_event(stratum_postgres::AppendEvent {
        agent_id: AgentId::from(agent_uuid),
        session_id: agent_state.session_id.expect("session bound"),
        turn_id: second_turn
            .parse::<uuid::Uuid>()
            .map(TurnId::from)
            .expect("turn uuid"),
        event: stratum_core::DurableAgentEvent::TranscriptCompacted {
            upto: 1,
            summary: stratum_core::ChatMessage::system(
                "[stratum:transcript-compacted]\nsummary of the prefix",
            ),
            compacted_iteration: 1,
        },
        approval_hook_invocation_id: None,
        default_model_update: None,
        compaction: Some(stratum_postgres::CompactionInput {
            compacted_iteration: 1,
            upto: 1,
            retained_from_event_seq: 2,
            summary: stratum_core::ChatMessage::system(
                "[stratum:transcript-compacted]\nsummary of the prefix",
            ),
        }),
    })
    .await
    .expect("compaction commits");

    let frontier = view(&fixture, &agent_id).await;
    let barrier = frontier["snapshot_event_seq"]
        .as_str()
        .expect("barrier")
        .to_owned();

    // Invalid windows.
    for uri in [
        format!("/v1/agents/{agent_id}/history"),
        format!("/v1/agents/{agent_id}/history?through_event_seq=abc"),
        format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}&before_event_seq=99999"),
        format!("/v1/agents/{agent_id}/history?through_event_seq=99999"),
        format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}&limit=0"),
        format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}&limit=300"),
        format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}&replay=all"),
    ] {
        let (status, body) = fixture.json("GET", &uri, None, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(body["error"]["code"], "invalid_history_query", "{uri}");
    }

    // First page of two, ascending.
    let (status, page) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}&limit=2"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let items = page["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(page["has_more"], true);
    let seqs: Vec<u64> = items
        .iter()
        .map(|item| {
            item["event_seq"]
                .as_str()
                .expect("string seq")
                .parse()
                .expect("decimal seq")
        })
        .collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));
    let before = page["next_before_event_seq"]
        .as_str()
        .expect("next cursor")
        .to_owned();

    // Older page through the cursor.
    let (status, older) = fixture
        .json(
            "GET",
            &format!(
                "/v1/agents/{agent_id}/history?through_event_seq={barrier}&before_event_seq={before}&limit=50"
            ),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let older_items = older["items"].as_array().expect("items");
    assert!(!older_items.is_empty());
    assert_eq!(older["has_more"], false);
    let oldest_seq: u64 = older_items
        .iter()
        .map(|item| {
            item["event_seq"]
                .as_str()
                .expect("string seq")
                .parse()
                .expect("decimal seq")
        })
        .max()
        .expect("max");
    assert!(oldest_seq < before.parse::<u64>().expect("cursor parses"));

    // The compaction marker carries the summary but never the pointer.
    let all = fixture
        .json(
            "GET",
            &format!("/v1/agents/{agent_id}/history?through_event_seq={barrier}&limit=50"),
            None,
            None,
        )
        .await
        .1;
    let marker = all["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["event"]["type"] == "transcript_compacted")
        .expect("compaction marker present");
    assert_eq!(marker["event"]["data"]["compacted_iteration"], 1);
    let raw = marker.to_string();
    assert!(!raw.contains("upto"));
    assert!(!raw.contains("retained_from_event_seq"));

    // Cleanup the running turn.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/cancel"),
            Some(json!({ "turn_id": second_turn })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&fixture, &agent_id, "cancelled").await;
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn request_bodies_are_capped_at_64_kib() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![]).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let big = "x".repeat(128 * 1024);
    let (status, body) = send_message(&fixture, &agent_id, &big, Value::Null).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "request_too_large");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn sse_streams_ready_then_durable_frames_with_cursors() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], MockProvider::text("streamed")).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();

    // Idle agent: subscription works, first frame is stream_ready without an
    // SSE id and without session/turn identity.
    let response = fixture
        .request(
            axum::http::Request::builder()
                .uri(format!("/v1/agents/{agent_id}/events"))
                .body(axum::body::Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let events = read_sse_events(response, 1).await;
    assert_eq!(events[0].data["kind"], "control");
    assert_eq!(events[0].data["event"]["type"], "stream_ready");
    assert_eq!(events[0].data["protocol_version"], 1);
    assert!(events[0].id.is_none());
    assert!(events[0].data.get("session_id").is_none());

    // Unknown agent and cursor validation happen before any header.
    let (status, body) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{}/events", uuid_v7()),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "agent_not_found");
    let (status, body) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{agent_id}/events?after_cursor=abc"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_cursor");
    let (status, body) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{agent_id}/events?replay=all"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");

    let invalid_header = axum::http::HeaderValue::from_bytes(&[0xff])
        .expect("opaque non-UTF-8 header value is valid HTTP");
    let response = fixture
        .request(
            axum::http::Request::builder()
                .uri(format!("/v1/agents/{agent_id}/events"))
                .header("Last-Event-ID", invalid_header)
                .body(axum::body::Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("error body reads");
    let body: Value = serde_json::from_slice(&bytes).expect("error response is json");
    assert_eq!(body["error"]["code"], "invalid_cursor");

    // Subscribe, then run a turn: durable frames arrive with cursor ids and
    // decimal-string event sequences.
    let response = fixture
        .request(
            axum::http::Request::builder()
                .uri(format!("/v1/agents/{agent_id}/events"))
                .body(axum::body::Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, accepted) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();
    let session_id = accepted["session_id"]
        .as_str()
        .expect("session id")
        .to_owned();
    let events = read_sse_until(response, 30, |event| {
        event.data["kind"] == "durable" && event.data["event"]["type"] == "loop_finished"
    })
    .await;
    assert_eq!(events[0].data["event"]["type"], "stream_ready");
    let durable: Vec<_> = events
        .iter()
        .filter(|event| event.data["kind"] == "durable")
        .collect();
    assert!(durable.len() >= 4, "durable frames arrive: {durable:?}");
    for frame in &durable {
        assert!(frame.id.is_some(), "durable frames carry cursor ids");
        assert_eq!(frame.data["session_id"], json!(session_id));
        assert_eq!(frame.data["turn_id"], json!(turn_id));
        assert!(
            frame.data["event_seq"]
                .as_str()
                .expect("string seq")
                .parse::<u64>()
                .is_ok()
        );
    }
    let seqs: Vec<u64> = durable
        .iter()
        .map(|frame| {
            frame.data["event_seq"]
                .as_str()
                .expect("string seq")
                .parse()
                .expect("decimal")
        })
        .collect();
    assert!(seqs.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        durable.last().expect("last").data["event"]["type"],
        "loop_finished"
    );
    // Telemetry frames share the tail and precede their final message.
    let telemetry: Vec<_> = events
        .iter()
        .filter(|event| event.data["kind"] == "telemetry")
        .collect();
    assert!(
        telemetry.len() >= 2,
        "llm telemetry streamed: {telemetry:?}"
    );
    for frame in telemetry {
        assert!(frame.id.is_some(), "telemetry frames carry cursor ids");
        assert!(
            frame.data["durable_before_event_seq"]
                .as_str()
                .expect("telemetry durable watermark is a string")
                .parse::<u64>()
                .is_ok(),
            "telemetry durable watermark is decimal"
        );
        assert!(frame.data["llm_call_id"].is_string());
        assert!(frame.data["telemetry_seq"].is_u64());
    }
    wait_for_status(&fixture, &agent_id, "finished").await;
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn sse_cursor_expiry_and_buffer_overflow() {
    use stratum_infra::{AgentTailConfig, NatsAgentTail};

    // Expiry: a tiny retention stream discards the cursor position.
    let tail = NatsAgentTail::connect(AgentTailConfig {
        url: common::nats_url(),
        stream_name: "AGENT_TAIL_EXPIRY".to_owned(),
        subject_prefix: "events.expiry".to_owned(),
        replicas: 1,
        max_age: std::time::Duration::from_secs(3600),
        max_bytes: 67_108_864,
        max_messages: 5,
    })
    .await
    .expect("expiry tail connects");
    let fixture = Fixture::with_tail(&[("agent-a", TEMPLATE)], vec![], Some(tail.clone())).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let agent_uuid = AgentId::from(agent_id.parse::<uuid::Uuid>().expect("agent uuid"));
    let mut first_cursor = None;
    for index in 0..8 {
        let cursor = tail
            .publish(
                &agent_uuid,
                bytes::Bytes::from(format!("{{\"index\":{index}}}")),
            )
            .await
            .expect("publish succeeds");
        first_cursor.get_or_insert(cursor);
    }
    let expired = first_cursor.expect("cursor").to_string();
    let (status, body) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{agent_id}/events?after_cursor={expired}"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(body["error"]["code"], "cursor_expired");

    // Overflow: publish more frames than the bounded buffer while the client
    // is not reading; the connection ends with a no-id stream_reset.
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![]).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let agent_uuid = AgentId::from(agent_id.parse::<uuid::Uuid>().expect("agent uuid"));
    let response = fixture
        .request(
            axum::http::Request::builder()
                .uri(format!("/v1/agents/{agent_id}/events"))
                .body(axum::body::Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let tail = NatsAgentTail::connect(AgentTailConfig {
        url: common::nats_url(),
        ..AgentTailConfig::default()
    })
    .await
    .expect("default tail connects");
    for index in 0..300 {
        tail.publish(
            &agent_uuid,
            bytes::Bytes::from(format!("{{\"index\":{index}}}")),
        )
        .await
        .expect("publish succeeds");
    }
    let events = read_sse_events(response, 400).await;
    let reset = events
        .iter()
        .find(|event| {
            event.data["kind"] == "control" && event.data["event"]["type"] == "stream_reset"
        })
        .expect("a stream_reset frame is emitted");
    assert!(reset.id.is_none(), "stream_reset never carries an SSE id");
    assert_eq!(reset.data["event"]["reason"], "buffer_overflow");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn realtime_unavailable_keeps_core_commands_working() {
    let fixture = Fixture::without_nats(&[("agent-a", TEMPLATE)], MockProvider::text("ok")).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();

    let (status, body) = fixture
        .json("GET", &format!("/v1/agents/{agent_id}/events"), None, None)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"]["code"], "realtime_unavailable");

    let (status, _) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&fixture, &agent_id, "finished").await;
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn health_and_not_found_endpoints() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![]).await;

    let (status, body) = fixture.json("GET", "/health/live", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    let (status, body) = fixture.json("GET", "/health/ready", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["realtime"], "ok");

    // Degraded realtime never fails readiness.
    let degraded = Fixture::without_nats(&[("agent-a", TEMPLATE)], vec![]).await;
    let (status, body) = degraded.json("GET", "/health/ready", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["realtime"], "degraded");

    // Unknown agents are 404 across read and command endpoints.
    let missing = uuid_v7();
    let (status, body) = fixture
        .json("GET", &format!("/v1/agents/{missing}"), None, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "agent_not_found");
    let (status, body) = send_message(&fixture, &missing, "hello", Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "agent_not_found");
    let (status, body) = fixture
        .json(
            "GET",
            &format!("/v1/agents/{missing}/history?through_event_seq=0"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "agent_not_found");

    // The OpenAPI document is served.
    let (status, body) = fixture
        .json("GET", "/api-docs/openapi.json", None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["paths"]["/v1/agents/{agent_id}/events"].is_object());
    assert!(body["components"]["schemas"]["AgentStreamFrameV1"].is_object());
    assert!(
        body["components"]["schemas"]["AgentViewResponse"]
            .to_string()
            .contains("telemetry_floor_event_seq"),
        "OpenAPI exposes the cold-recovery telemetry floor"
    );
    assert!(
        body["components"]["schemas"]["AgentStreamFrameV1"]
            .to_string()
            .contains("durable_before_event_seq"),
        "OpenAPI exposes the telemetry durable watermark"
    );

    // Malformed path identities are 400.
    let (status, body) = fixture
        .json("GET", "/v1/agents/not-a-uuid", None, None)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn startup_fails_when_the_template_root_is_missing() {
    let missing = std::env::temp_dir().join(format!("stratum-api-missing-{}", uuid_v7()));
    let config = stratum_config::Config::parse(&format!(
        r#"
[agent]
templates_root = {root:?}

[llm]
default = "openai:test-model"

[llm.openai]
api_key = "test-key"
models = ["test-model"]

[postgres]
url = "postgres://unused:unused@127.0.0.1:1/unused"
"#,
        root = missing.to_string_lossy()
    ))
    .expect("config parses");
    let pg = PostgresBackend::connect(&common::pg_url())
        .await
        .expect("postgres connects");
    let result =
        stratum_api::AppState::new(pg, None, stratum_llm::LlmProviderManager::new(), config).await;
    assert!(
        matches!(result, Err(stratum_api::HostError::TemplatesRoot(_))),
        "a missing template root fails startup"
    );
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn stale_expected_turn_wins_over_busy_and_resume_required() {
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], vec![Script::Pending]).await;
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_id = created["agent_id"].as_str().expect("agent id").to_owned();
    let (status, accepted) = send_message(&fixture, &agent_id, "hello", Value::Null).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let turn_id = accepted["turn_id"].as_str().expect("turn id").to_owned();

    // Hosted running turn: a retry with the OLD expected value is stale (it
    // must never create a second turn), while the correct current value still
    // reports busy.
    let (status, body) = send_message(&fixture, &agent_id, "again", Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");
    let (status, body) = send_message(&fixture, &agent_id, "again", json!(turn_id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "agent_busy");

    // Unhosted running turn (restarted host): stale still wins over
    // resume_required, and the correct current value still requires resume.
    let restarted = restarted(&fixture, vec![]).await;
    let (status, body) = send_message(&restarted, &agent_id, "again", Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "stale_turn");
    let (status, body) = send_message(&restarted, &agent_id, "again", json!(turn_id)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "resume_required");

    // Clean up the running turn through the original host.
    let (status, _) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_id}/cancel"),
            Some(json!({ "turn_id": turn_id })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    wait_for_status(&fixture, &agent_id, "cancelled").await;
}

/// Commits one running turn whose `LoopStarted` snapshot is the real snapshot
/// of `source_turn` with one field tampered, plus its first user message, and
/// returns the new turn id.
async fn begin_tampered_turn(
    pg: &PostgresBackend,
    agent_id: &str,
    source_turn: &str,
    tamper: impl FnOnce(&mut TurnRuntimeSnapshot),
) -> String {
    let agent_uuid = agent_id.parse::<uuid::Uuid>().expect("agent uuid");
    let source_uuid = source_turn.parse::<uuid::Uuid>().expect("turn uuid");
    let started = pg
        .read_loop_started(AgentId::from(agent_uuid), TurnId::from(source_uuid))
        .await
        .expect("source loop_started reads");
    let mut snapshot = started.snapshot;
    tamper(&mut snapshot);
    let turn_id = TurnId::new();
    pg.begin_turn(BeginTurn {
        agent_id: AgentId::from(agent_uuid),
        expected_current_turn_id: Some(TurnId::from(source_uuid)),
        turn_id,
        session_id: started.session_id,
        snapshot,
    })
    .await
    .expect("tampered turn commits");
    pg.append_event(stratum_postgres::AppendEvent {
        agent_id: AgentId::from(agent_uuid),
        session_id: started.session_id,
        turn_id,
        event: stratum_core::DurableAgentEvent::MessageAppended {
            message: stratum_core::ChatMessage::user("hi"),
        },
        approval_hook_invocation_id: None,
        default_model_update: None,
        compaction: None,
    })
    .await
    .expect("first user message commits");
    turn_id.to_string()
}

#[tokio::test]
#[ignore = "requires the stratum-api-test compose stack"]
async fn resume_rejects_snapshot_skill_and_hook_mismatches() {
    let mut script = MockProvider::text("one");
    script.extend(MockProvider::text("two"));
    let fixture = Fixture::new(&[("agent-a", TEMPLATE)], script).await;
    let pg = PostgresBackend::connect(&common::pg_url())
        .await
        .expect("postgres connects");

    // Agent A: the persisted skill set identity does not match the runtime
    // this binary rebuilds.
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_a = created["agent_id"].as_str().expect("agent id").to_owned();
    let (_, accepted) = send_message(&fixture, &agent_a, "first", Value::Null).await;
    let turn_a = accepted["turn_id"].as_str().expect("turn id").to_owned();
    wait_for_status(&fixture, &agent_a, "finished").await;
    let tampered = begin_tampered_turn(&pg, &agent_a, &turn_a, |snapshot| {
        snapshot.skill_set_version_id = SkillSetVersionId::new();
    })
    .await;
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_a}/resume"),
            Some(json!({ "turn_id": tampered })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], "runtime_unavailable");

    // Agent B: the persisted ordered hook handler versions do not match the
    // chain this binary rebuilds.
    let (_, created) = create_agent(&fixture, &uuid_v7(), "agent-a").await;
    let agent_b = created["agent_id"].as_str().expect("agent id").to_owned();
    let (_, accepted) = send_message(&fixture, &agent_b, "first", Value::Null).await;
    let turn_b = accepted["turn_id"].as_str().expect("turn id").to_owned();
    wait_for_status(&fixture, &agent_b, "finished").await;
    let tampered = begin_tampered_turn(&pg, &agent_b, &turn_b, |snapshot| {
        snapshot.hook_handler_versions = Vec::new();
    })
    .await;
    let (status, body) = fixture
        .json(
            "POST",
            &format!("/v1/agents/{agent_b}/resume"),
            Some(json!({ "turn_id": tampered })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["code"], "runtime_unavailable");
}
