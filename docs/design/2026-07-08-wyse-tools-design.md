# wyse-tools Design

Date: 2026-07-08

## Goal

Add a small `wyse-tools` crate that defines the runtime tool boundary used by
future agent code. Agents will receive a pre-populated tool registry when they
are created, then call tools through that registry.

The first implementation only needs one builtin tool to prove the boundary.

## Type Ownership

- `wyse-core` owns cross-crate protocol primitives.
- Add `ToolName` to `wyse-core` as a string newtype.
- Move `ToolSpec` to `wyse-core` because both LLM requests and runtime tools
  need the same provider-visible name, description, and JSON input schema.
- `wyse-llm` continues to re-export `ToolSpec` for compatibility and keeps
  `ToolCall` and `ToolCallDelta`, because those are LLM provider output types.
- `wyse-tools` owns execution-time types:
  - `ToolInput { call_id: CallId, arguments: serde_json::Value }`
  - `ToolOutput { result: serde_json::Value }`

`ToolInput` and `ToolOutput` stay out of `wyse-core` until another crate needs
them directly.

## Traits

Use `async_trait::async_trait` for object-safe async traits. This adds one small
dependency, but keeps future agent injection simple with `Arc<dyn ToolRegistry>`.

`Tool: Send + Sync`:

- `fn spec(&self) -> &ToolSpec`
- `async fn call(&self, input: ToolInput) -> Result<ToolOutput, ToolError>`

`ToolRegistry: Send + Sync`:

- `fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), ToolError>`
- `fn get(&self, name: &ToolName) -> Option<Arc<dyn Tool>>`
- `fn specs(&self) -> Vec<ToolSpec>`
- `async fn call(&self, name: &ToolName, input: ToolInput) -> Result<ToolOutput, ToolError>`

The registry owns registered tools by `Arc` so agent code can cheaply hold and
share a registry behind `Arc<dyn ToolRegistry>`. The `specs` method gives agent
code the provider-visible tool list for `ChatRequest.tools` without exposing the
registry internals.

## Builtin First Version

Add a concrete builtin registry backed by a map from `ToolName` to `Arc<dyn Tool>`.
Registering a duplicate tool name returns a typed error.

Add one builtin tool:

- `EchoTool`: returns its JSON arguments as `ToolOutput.result`.

This is only a boundary test tool. It does not imply a permanent public utility
surface.

## Errors

`wyse-tools` is a library crate, so it uses `thiserror`.

Initial errors:

- duplicate tool registration
- tool not found

Messages must start lowercase and preserve source chains when present.

## Tests

Add focused unit tests in the new crate:

- duplicate registration fails
- registered echo tool can be called through the registry
- missing tool returns a typed error

Run `cargo fmt` and `cargo test --workspace --all-targets`.

## Explicitly Skipped

- shell tools
- file tools
- permissions or sandbox policy
- dynamic plugin loading
- provider-specific tool schema shaping
- internal tool id mapping beyond `ToolName`
- factories, managers, or registries of registries

Add those only when a real caller needs them.
