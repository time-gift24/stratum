# Host model selection and parameter configuration design

## Status

Approved during the 2026-07-12 design discussion. This document scopes the
backend only; the web UI will consume the API in a later change.

## Goals

- Let a message select a complete model configuration for the accepted turn
  and all later turns of the same agent.
- Persist the currently effective model configuration in `agent.json` so a
  restart or explicit resume uses the same configuration.
- Let clients discover every configured model and its provider-defined
  parameter schema through the host API.
- Keep provider-specific parameters validated and represented as strong types
  inside their provider implementations.

## Non-goals

- Do not change a running turn's model or parameters.
- Do not add model parameters for OpenAI in this change.
- Do not add a frontend model picker or parameter form.
- Do not make templates mutable configuration for an existing agent.

## Model configuration

`wyse-core` will expose a public `ModelConfig` value with:

- `model: ModelId`
- `parameters`: a JSON object

It is the stable API and persistence snapshot. It is intentionally generic at
the host boundary because a provider owns its own parameter vocabulary. The
host rejects a non-object `parameters` value before invoking a provider.

The initial configuration for a newly created agent uses the resolved template
model plus that model's provider-defined default parameters. The template and
the persisted `definition.toml` remain the original template definition; they
do not override an existing agent's current configuration.

## Provider configuration boundary

`wyse-llm` will add a narrow `ConfigurableLlmProvider` trait alongside
`LlmProvider`. A configurable provider is registered by `ModelId` and must:

1. return a JSON Schema for its parameters;
2. return a complete default parameter object; and
3. validate a complete parameter object and construct a configured
   `Arc<dyn LlmProvider>`.

`LlmProviderManager` will store/configure these providers rather than exposing
provider-specific branches to the host. A configured provider reports the
exact model configuration it is bound to, allowing the agent's durable
start-turn transition to persist the same setting that will execute the turn.

DeepSeek accepts exactly these parameters:

```json
{
  "thinking": { "type": "disabled" }
}
```

or:

```json
{
  "thinking": {
    "type": "enabled",
    "reasoning_effort": "high"
  }
}
```

`reasoning_effort` is optional when thinking is enabled and, when supplied,
is `"high"` or `"max"`. The provider maps it to the existing
`DeepSeekThinking` and `DeepSeekReasoningEffort` types. OpenAI exposes an
empty-object schema and rejects non-empty parameters.

## HTTP API

### List models

`GET /v1/models` returns the models registered by the host, in deterministic
model-id order. Each item contains:

```json
{
  "model": "deepseek:deepseek-v4-pro",
  "parameters_schema": {
    "type": "object",
    "description": "provider-defined complete parameter object"
  },
  "default_parameters": { "thinking": { "type": "disabled" } }
}
```

No credential, endpoint, raw provider object, or unconfigured model is
returned.

### Send a message

`POST /v1/agents/{agent_id}/messages` gains this optional field:

```json
{
  "text": "continue",
  "model_config": {
    "model": "deepseek:deepseek-v4-pro",
    "parameters": {
      "thinking": { "type": "enabled", "reasoning_effort": "high" }
    }
  }
}
```

When omitted, the host uses the configuration currently in `agent.json`. When
present, it is a complete replacement snapshot, never a patch. The agent must
be idle and must not require resume. A running or resumable agent returns the
existing conflict response and leaves the configuration unchanged.

`GET /v1/agents/{agent_id}` adds the current `model_config` to `AgentView`.

## Durable transition and recovery

`AgentState` gains `model_config`, and the serialized state format version is
advanced without adding separately named state-version types. `AgentStore`
adds a start-turn operation that atomically transitions an idle agent to
`running` and records the configured provider's complete `ModelConfig`.

This establishes one durable acceptance boundary: a successful message
response means both the user message turn and its selected configuration were
accepted. A model validation failure, a busy conflict, or a failed turn start
cannot leave a newly selected configuration behind.

The host validates and composes a candidate configured agent before starting a
turn. After its durable start succeeds, it becomes the hosted agent used for
cancellation, completion, and subsequent messages. Per-agent transition
coordination must not hold a `Mutex` or `RwLock` guard across I/O; the registry
lock remains limited to the in-memory agent map.

During restore, the host composes each agent from its persisted `model_config`.
If a legacy state lacks this field, the host reads its durable
`definition.toml`, asks the registered provider for default parameters, and
atomically writes the completed state before loading history or resuming. A
persisted model that is no longer configured, or persisted parameters rejected
by its provider, fails whole-host restoration rather than returning a partial
registry.

## Errors

- Malformed request JSON or a non-object `parameters` value is a stable 400
  request error.
- An unavailable model or provider-rejected parameter object is a stable 422
  validation error.
- An active agent or one requiring resume returns the existing 409 conflict
  behavior without state mutation.
- A closed host returns the existing 503 shutdown behavior without state
  mutation.

Errors and tracing must not include parameter payloads, prompts, API keys, or
provider credentials.

## Verification

- Unit-test `ModelConfig` serialization and the object invariant.
- Unit-test configurator schemas, defaults, and invalid parameter rejection
  for DeepSeek and OpenAI.
- Extend DeepSeek protocol tests for disabled thinking, enabled thinking, and
  both allowed effort levels.
- Test provider-manager model listing and configured-provider construction.
- Test API model discovery, `AgentView` projection, initial defaults, omitted
  configuration reuse, model switching, and DeepSeek reasoning changes.
- Test that invalid parameters, an unavailable model, busy state, required
  resume, and shutdown do not change persisted configuration.
- Test restart recovery and legacy-state completion, including an interrupted
  running turn.

Before merging, document the resulting crate-level invariants in the affected
crate `AGENTS.md` files as required by the repository policy.
