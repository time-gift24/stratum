# Wyse Agent OS TODO

## Crates

- [ ] `wyse-core`
  - 核心 ID：`RunId`、`AgentId`、`ToolId`、`ModelId`
  - 共享 error/result 类型
  - 通用 value model 和 JSON schema helpers

- [ ] `wyse-infra`
  - 内部 event bus
  - runtime event 和 trace primitives
  - clock/time provider
  - ID generator
  - cancellation token helpers
  - retry/backoff utilities
  - rate-limit primitives
  - permission context 和 policy hooks
  - config/env loading helpers

- [ ] `wyse-llm`
  - 统一 chat、completion、streaming、embedding、rerank 抽象
  - 优先实现 OpenAI-compatible provider
  - 测试用 mock provider
  - tool-call normalization
  - structured output 支持
  - token/cost primitives

- [ ] `wyse-tools`
  - 内部 `Tool` trait
  - tool registry
  - tool input/output schema validation
  - local tool adapter
  - HTTP/OpenAPI tool adapter
  - tool execution trace 和 errors

- [ ] `wyse-mcp`
  - MCP protocol types
  - MCP client
  - MCP server
  - MCP transport 支持：stdio 优先，HTTP 后续
  - MCP tools 到 `wyse-tools` 的 adapter
  - Wyse tools/workflows/agents 到 MCP server tools 的 adapter
  - MCP permission、allowlist、timeout、audit hooks

- [ ] `wyse-agent`
  - agent runtime loop
  - 优先实现 function-calling agent strategy
  - 后续实现 ReAct strategy
  - 通过 `wyse-tools` 编排 tool-use
  - 通过 `wyse-llm` 访问模型
  - step budget、timeout、cancellation
  - agent memory interface

- [ ] `wyse-workflow`
  - workflow DSL types
  - workflow node IDs 和 edge references
  - DAG validation
  - node runtime trait
  - 优先实现 in-memory graph executor
  - 后续实现 queue-based executor
  - built-in nodes：start、output、llm、tool、agent、if、loop、parallel、human-input
  - run state、node state、resume/cancel 支持

- [ ] `wyse-knowledge`
  - document ingestion pipeline
  - parser/chunker interfaces
  - 通过 `wyse-llm` 做 embedding
  - vector store abstraction
  - retriever 和 reranker
  - citation/source metadata

- [ ] `wyse-store`
  - persistence traits
  - workflow definitions
  - run snapshots
  - event log
  - secrets references
  - SQLite 优先，Postgres 后续

- [ ] `wyse-api`
  - Axum HTTP API
  - workflow run API
  - agent invoke API
  - SSE streaming events
  - human-input resume API
  - admin/debug endpoints

- [ ] `wyse-cli`
  - validate workflow files
  - 本地运行 workflow
  - 本地 invoke agent
  - list tools/models/MCP servers
  - debug run events

## 初始实现顺序

1. [ ] 创建 Rust workspace 和 crate skeletons
2. [ ] 实现 `wyse-core`
3. [ ] 实现 `wyse-infra`
4. [ ] 实现 `wyse-llm`，包含 mock provider 和 OpenAI-compatible provider
5. [ ] 实现 `wyse-tools`，包含 local tools
6. [ ] 实现 `wyse-mcp` client，包含 stdio tool adapter
7. [ ] 实现 `wyse-agent` function-calling loop
8. [ ] 实现 `wyse-workflow` in-memory DAG runtime
9. [ ] 实现 `wyse-store` SQLite persistence
10. [ ] 实现 `wyse-api` REST + SSE
11. [ ] 实现 `wyse-cli` commands
12. [ ] 实现 `wyse-knowledge`
13. [ ] 添加 MCP server mode

## 第一个可运行里程碑

- [ ] `wyse-cli chat --model <model>`
- [ ] `wyse-cli tool call <tool> --json <args>`
- [ ] `wyse-cli mcp list-tools <server>`
- [ ] `wyse-cli agent run <agent.yaml>`
- [ ] `wyse-cli workflow run <workflow.yaml>`
