# Alpha 人工端到端验收入口

## 目的与范围

本文档是 `complete-postgres-agent-runtime` 当前 Alpha 版本的人工端到端验收入口。它只验证现有 production-like Compose 装配能够真实到达的用户路径：Postgres 是唯一 durable truth，NATS 只提供短期实时 tail，Web 最终通过 Postgres view/history 收敛。

本文档不是 HTTP、事件或存储协议的第二份定义。行为冲突时依次以 OpenSpec、utoipa 生成的 OpenAPI 和相关 crate `AGENTS.md` 为准。

本轮人工验收只包含 8 条互相独立的 journey：

1. Compose health、catalog 与 Web；
2. 普通 LLM Turn；
3. 浏览器硬刷新后的 Postgres 恢复；
4. Echo approval 的 approve/reject；
5. approval pending 时重启 API，再 refresh、resolve 与显式 resume；
6. hosted running Turn 的 pending cancel；
7. 超过一页的 history 向上加载；
8. 同一 AgentId 下多个 AgentRuntime 的隔离。

以下内容不在本文档中执行：

- 精确 COMMIT 前后窗口、commit acknowledgement 不确定性；
- Tool crash/at-least-once、进程级精确暂停点；
- NATS retention、cursor expiry、buffer overflow、慢队列；
- SQL corruption、非法 durable shape、compaction pointer 损坏；
- compaction 线上触发与 summary Hook。当前 production composition 尚未注册 compaction Hook。

当前 stock Compose 可执行的两个外部故障——F01 NATS stop/recover、F02 Postgres stop/recover——见 [ALPHA_FAULT_INJECTION.md](ALPHA_FAULT_INJECTION.md)。上面列出的精确 COMMIT、Tool crash、hosted slow/pending Turn 上的 API SIGTERM、slow/full/expiry/overflow 与 SQL corruption 场景延期到 [P4a](CONTEXT.html#todo/p4a)；production compaction 策略与 Hook 延期到 [H5b](CONTEXT.html#todo/h5b)，对应产品/故障验收延期到 [H5c](CONTEXT.html#todo/h5c)。完成本文档不会自动完成 OpenSpec `10.4` 或 `10.12`。

已经能由单元测试或 crate-local ignored integration test 确定性覆盖的行为，不在这里重复手工模拟。本文档只补真实 Compose 进程、真实浏览器和真实 provider 边界的证据。

## 安全边界

1. 只使用可丢弃的 Alpha Postgres database、NATS stream、Agent template 和 AgentRuntime。不得连接开发共享库、生产库或用户历史。
2. 当前是单进程、单一可信操作者的 Alpha；入站 API 没有 auth/authz 或 tenant isolation。API 只能绑定 loopback/受控私网，或置于带 TLS 与认证的反向代理后。Postgres 与 NATS 端口不得暴露公网。
3. 只使用合成测试数据。prompt、Echo arguments/result、approval 和完整 conversation 会持久化；当前没有 delete API。真实 LLM 会外发上下文并产生费用，只能使用低权限、限额测试 key。
4. 真实 provider key、token、数据库连接凭据和本地 secret 配置不得加入 Git。证据中不得出现它们的值。
5. 证据不得保存原始 prompt、assistant 正文、reasoning、Echo arguments/result、provider body、SQL connection string 或 credential。截图必须裁切或遮盖正文，只保留状态、控件和安全 identity。
6. 本清单禁止直接修改 SQL、删除 NATS stream、缩短 retention、安装 trigger、增加 failpoint/debug endpoint，或修改生产 buffer 常量。这些精确测试能力尚未实现，统一延期到 `CONTEXT.html` 工程待办中的 P4a；production compaction 策略/Hook 与其产品故障验收分别延期到 H5b/H5c，不得借 F01—F02 扩大范围。
7. 每条 journey 都创建自己的 fresh AgentRuntime；不得复用其他 journey 的 AgentRuntime、Session、Turn、Approval 或浏览器恢复状态。某条 journey 的结果不得作为另一条的 fixture。
8. J04 的 approve 与 reject 分别使用两个 fresh AgentRuntime，避免第一条路径留下的 Turn/history 影响第二条。
9. journey 之间可以复用同一健康的 disposable Compose stack，但开始前必须重新确认服务身份与 health；任何被手工篡改或发生未知状态的 stack 都必须丢弃重建。
10. 当前只注册无 credential 通道的 `echo` Tool。不得为 Alpha 验收注册 credential-aware Tool，也不得在宿主机执行 agent 生成的命令。
11. crash 后的 running Turn 需要显式 resume。approval resolve 与 resume 是两个动作；resolve 不得隐式接管 Turn。
12. cancel `202` 只表示本进程内的 cancellation token 已 signal；在 durable terminal 提交前不得把 UI 或证据写成 `cancelled`。正常完成可以赢得竞态。
13. 本清单不测试或承诺 scheduler lease/fencing、多实例接管、rolling deploy、自动 resume、durable cancel、并发 Tool、通用 Tool 幂等、Workflow 协调或 NATS durable backlog。

## 通用环境与 fresh runtime 约定

本文将 Compose 实现统称为“Compose”。仓库本地默认使用 `podman compose`；已安装 Docker CLI 时可以等价使用 `docker compose`。每轮证据必须记录实际命令，不得假定执行机一定安装 Docker。

### 本地 provider secret 配置

根 `docker-compose.yml` 默认挂载带占位 key 的 tracked `config.docker.toml`，而当前配置解析器不会用 `DEEPSEEK_API_KEY` 自动覆盖 TOML。发送真实消息前，必须把已存在于安全运行环境的 key 写入 Git 忽略的 owner-only 配置，并通过本地 Compose override 只读挂载；禁止修改或提交 tracked 示例配置。

以下命令不打印 key，也不把 key 放进子进程参数；如果环境变量为空会直接失败：

```sh
test -n "${DEEPSEEK_API_KEY:-}" || { echo "DEEPSEEK_API_KEY is required" >&2; exit 1; }
umask 077
mkdir -p .stratum/alpha
python3 - <<'PY'
import json
import os
from pathlib import Path

source = Path("config.docker.toml").read_text(encoding="utf-8")
placeholder = 'api_key = "replace-with-deepseek-api-key"'
if source.count(placeholder) != 1:
    raise SystemExit("expected exactly one Docker API-key placeholder")
key = os.environ["DEEPSEEK_API_KEY"]
if not key.strip():
    raise SystemExit("DEEPSEEK_API_KEY is empty")
rendered = source.replace(placeholder, f"api_key = {json.dumps(key)}")
Path(".stratum/alpha/config.toml").write_text(rendered, encoding="utf-8")
PY
chmod 600 .stratum/alpha/config.toml
cat > .stratum/alpha/compose.override.yml <<'YAML'
services:
  stratum-api:
    volumes:
      - ./.stratum/alpha/config.toml:/app/config.toml:ro
YAML
```

启动本轮基础 journey 时固定同时传入两个 Compose 文件：

```sh
podman compose \
  -f docker-compose.yml \
  -f .stratum/alpha/compose.override.yml \
  up -d --build --wait
```

Docker 用户只把 `podman` 替换为 `docker`。不得运行会输出渲染后配置正文的命令，也不得把 `.stratum/alpha/config.toml` 收入证据；整轮结束后可安全删除本地 `.stratum/alpha/`。

默认入口：

- Web：`http://127.0.0.1:5173/conversation`
- API：`http://127.0.0.1:18080`
- API liveness：`/health/live`
- API readiness：`/health/ready`
- Template catalog：`/v1/agent-templates`
- Model catalog：`/v1/models`
- NATS monitor：`http://127.0.0.1:8222`

每条 journey 开始时都必须：

1. 记录 Git commit、Compose project、四个服务的容器身份与版本。
2. 独立确认 Postgres、NATS、API 与 Web 健康；不得把 J01 的 PASS 当作后续 journey 的健康证明。
3. 确认 API 使用有效的低权限 provider credential，但不记录 credential 值。
4. 从一个没有 selected runtime 的 Web“新对话”状态开始。J01 是唯一直接调用 create API 的 journey，因此只为 J01 生成新的 UUID `Idempotency-Key`。
5. J02—J08 不预先调用 create API：当前 Web 会在首条消息中先创建 AgentRuntime、再向它发送首个 message；每个新对话都必须让 Web 生成新的创建 intent/key。J04 和 J08 各使用两个独立的新对话。
6. runtime 一旦建立，立即记录 `AgentRuntimeId` 与 pinned `AgentId`；后续只操作这一 exact runtime。只有 J01 要求观察纯 create 后的 `idle`、无 Session/current Turn、`last_event_seq=0`；其余 journey 的首次可见 durable 状态可以已经是 `running` 或 terminal。
7. journey 完成后停止继续写入该 runtime，避免证据屏障变化。

如果 journey 因 provider 时序或输出而没有进入所需状态，例如 cancel 请求到达前 Turn 已结束，或模型没有产生所要求的 Echo Tool call，应把本次尝试记为未执行，并用另一个 fresh AgentRuntime 重试；不得在旧 runtime 上伪造前置状态，也不得用临时 Hook/mock 冒充 production composition。

## 统一证据模板

每条 journey 都填写：

| 字段 | 值 |
|---|---|
| Journey |  |
| Git commit |  |
| 日期 / 执行人 |  |
| Compose 命令与 project |  |
| API / Web 地址 |  |
| Postgres / NATS / API / Web 容器身份 |  |
| 浏览器 / OS |  |
| Provider / model（无 key） |  |
| AgentRuntimeId |  |
| AgentId / template name / version |  |
| SessionId / TurnId |  |
| ApprovalId（适用时） |  |
| Evidence 目录或 CI URL |  |
| 结果（PASS/FAIL/BLOCKED） |  |
| Issue / 备注 |  |

每条 journey 至少保存以下安全证据：

- 带时间戳的操作顺序；
- 相关 HTTP status 与稳定 `error.code`，不保存敏感 response body；
- AgentRuntimeId、AgentId、SessionId、TurnId、ApprovalId；
- 操作前后 `agent_states.status/current_turn_id/last_event_seq`；
- 按 `event_seq` 排序的安全 event type 清单，以及 sequence 是否连续；
- 浏览器最终状态，以及是否出现重复消息、ghost draft、错误 pending 状态或伪造 terminal；
- 如发生重启，记录重启前后的 API 容器身份及最终恢复结果。

可使用以下只读查询记录安全证据。禁止在证据查询中选择 `payload`、`runtime_snapshot`、`resolved_definition` 或 conversation 正文：

```sql
SELECT s.id AS agent_runtime_id, s.agent_id, a.name, a.version,
       s.status, s.session_id, s.current_turn_id, s.last_event_seq
FROM agent_states AS s
JOIN agents AS a ON a.id = s.agent_id
WHERE s.id = '<agent_runtime_uuid>';

SELECT event_seq, turn_id, event_type, event_version
FROM durable_events
WHERE agent_runtime_id = '<agent_runtime_uuid>'
ORDER BY event_seq;

SELECT expected.event_seq AS missing_event_seq
FROM generate_series(
  1,
  (SELECT last_event_seq
   FROM agent_states
   WHERE id = '<agent_runtime_uuid>')
) AS expected(event_seq)
LEFT JOIN durable_events AS actual
  ON actual.agent_runtime_id = '<agent_runtime_uuid>'
 AND actual.event_seq = expected.event_seq
WHERE actual.event_seq IS NULL;
```

通用通过条件：

- Postgres 始终是 durable truth；NATS frame 与浏览器临时状态不能覆盖它。
- AgentRuntime-wide `event_seq` 从 1 开始、严格递增且无洞。
- 所有请求、SSE frame 和页面状态保持 exact AgentRuntimeId 与 pinned AgentId。
- J01 的纯 create 不创建 Session、Turn 或 durable event；J02—J08 的 Web 首发会在创建后立即开始首个 Turn，这是产品既定流程。
- durable terminal 至多一个；页面最终状态与 Postgres 一致。
- 错误保持 typed、fail-closed，日志和证据不泄露敏感信息。

## Journey 索引

| ID | Journey | Fresh runtime 数 | 主要验证面 | 结果 |
|---|---|---:|---|---|
| J01 | Compose health、catalog 与 Web | 1 | 部署入口与创建 | [x] |
| J02 | 普通 LLM Turn | 1 | message、SSE、durable terminal | [x] |
| J03 | 硬刷新后的 Postgres 恢复 | 1 | cold bootstrap 与去重 | [x] |
| J04 | Echo approval approve/reject | 2 | 两种审批决定 | [x] |
| J05 | Pending approval 跨 API 重启恢复 | 1 | refresh、resolve、explicit resume | [x] |
| J06 | Pending cancel 等待 durable terminal | 1 | 内存级 cancel 语义 | [x] |
| J07 | 超过一页的 history | 1 | 向上分页 | [x] |
| J08 | 同 AgentId 多 AgentRuntime 隔离 | 2 | identity、ledger、realtime 隔离 | [x] |

## J01 — Compose health、catalog 与 Web

### 独立前置

- 使用当前 commit 启动 disposable Compose stack。
- 按通用约定准备本 journey 专属的 `Idempotency-Key`，但不要预先创建 AgentRuntime。

### 操作

1. 检查 Postgres 与 NATS container health，以及 API、Web container 状态。
2. 请求 API liveness 与 readiness；确认 Postgres core ready，NATS realtime 未 degraded。
3. 读取 template catalog 与 model catalog。
4. 打开 Web conversation 页面，确认 template/model 可选择且页面没有启动错误。
5. 直接调用 `POST /v1/agent-runtimes` 创建一个 fresh AgentRuntime，并读取其 AgentRuntimeView；不要发送 message。

### 预期

- 四个服务均健康；API liveness/readiness 与 Web 页面可访问。
- catalog 至少包含本轮使用的 template `name/version` 与可用 model，且不暴露 prompt、raw TOML、path 或 credential。
- create response 与 AgentRuntimeView 返回新的 AgentRuntimeId、正确的 pinned AgentId 及 template name/version；Web 只需证明 catalog 与空白 conversation 页面可用，不要求显示内部 UUID。
- runtime 保持 `idle`、无 Session/current Turn、`last_event_seq=0`；数据库中不存在该 runtime 的 durable event。

## J02 — 普通 LLM Turn

### 独立前置

- 独立确认 stack health。
- 在 Web 打开新的空白对话并选择 template/model；不得预先创建 runtime，也不得使用 J01 的 runtime。

### 操作

1. 从 Web 发送一条简短的合成消息，不要求 Tool；该首发必须创建本 journey 的 fresh AgentRuntime。
2. 从 Network/API 记录 create 与 message command 的 accepted 状态，以及 AgentRuntimeId、pinned AgentId、SessionId/TurnId。
3. 观察当前 Turn 的 realtime draft，并等待完整 assistant message 和 durable terminal。
4. 读取最终 AgentRuntimeView 与安全 ledger event type 清单。

### 预期

- message 被接受后，runtime 绑定唯一 Session/current Turn，并进入 `running`。
- Web 可以显示增量 draft；draft 最终由完整 durable assistant message 替换，不残留 ghost draft。
- ledger 只有一个 `LoopStarted`，包含 user 与 assistant 的 `MessageAppended`，并最终存在唯一 `LoopFinished`。
- 最终 view 为 `finished`，usage、barrier 与页面状态收敛到同一 Postgres truth。
- event sequence 连续，SSE/NATS 丢失任何非必要 telemetry 都不会改变最终 durable 结果。

## J03 — 硬刷新后的 Postgres 恢复

### 独立前置

- 独立确认 stack health。
- 在 Web 新建空白对话，以首条普通消息创建本 journey 的 fresh AgentRuntime并完成该 Turn；不得引用 J02 的结果。

### 操作

1. 在 Turn 完成后记录 AgentRuntimeId、AgentId、SessionId、TurnId、最终 status 与 `last_event_seq`。
2. 记录页面当前消息数量和是否存在 draft/pending UI。
3. 对 conversation 页面执行浏览器硬刷新。
4. 刷新后 Web 不会自动恢复 selected runtime；从左侧 recent conversation 列表显式重新选择原 AgentRuntimeId。
5. 等待页面完成 fresh subscription 与 Postgres cold bootstrap，再次读取 view/history。

### 预期

- 刷新后 recent 列表仍可找到原 AgentRuntime；显式重选不会创建替代 runtime、Session 或 Turn。
- 页面从 Postgres view/history 恢复完整 durable conversation 与 terminal 状态，不依赖 NATS 重放旧历史。
- 同一 durable message 不重复出现；不存在 ghost draft、过期 approval、错误 cancel pending 或伪造 terminal。
- 在无新写入时，刷新前后 `last_event_seq` 相同且 ledger 连续。
- 新页面生命周期不复用刷新前的内存 buffer 或 page cursor。

## J04 — Echo approval approve/reject

### 独立前置

- 独立确认 stack health。
- 准备两个独立的 Web 新对话：一个只用于 approve，一个只用于 reject。两者都选择声明 `echo` Tool 的同一 template，且都不得被其他 journey 使用。
- 每个新对话的首条 Tool 消息分别创建自己的 fresh AgentRuntime；不要预先调用 create API。

### 操作

#### J04-A：Approve

1. 在 approve 新对话发送明确要求调用一次 `echo` 的合成消息，并记录由该首发创建的 AgentRuntimeId。
2. 等待 Web 显示 pending approval，记录 ApprovalId、CallId、SessionId 与 TurnId。
3. 点击 approve 一次，等待 Turn 继续并到达 durable terminal。

#### J04-R：Reject

1. 在 reject 新对话发送明确要求调用一次 `echo` 的合成消息，并记录由该首发创建的 AgentRuntimeId。
2. 等待 Web 显示 pending approval并记录安全 identity。
3. 点击 reject 一次，等待模型收到 blocked Tool result 后继续到 durable terminal。

### 预期

- 两条路径各自只有一个 `ToolApprovalRequested` 与一个 `ToolApprovalResolved`，pending UI 在 resolve 后消失。
- approve 路径存在同 CallId 的 `ToolExecutionStarted`，随后提交 Tool result message并继续。
- reject 路径不出现 `ToolExecutionStarted`；拒绝作为模型可见的安全 Tool result 继续，不执行 Echo。
- 两条路径都只产生一个 terminal，且各自的 approval、Turn 与 event sequence 不跨 runtime 混用。
- 页面可以为当前可信操作者展示合成的 Echo arguments/result；截图、日志摘录和其他持久证据必须裁切或遮盖这些正文。

## J05 — Pending approval 跨 API 重启恢复

### 独立前置

- 独立确认 stack health。
- 在 Web 打开新的空白对话并选择声明 `echo` Tool 的 template；首条 Tool 消息将创建本 journey 的 fresh AgentRuntime。
- 本 journey 只重启 `stratum-api`；必须保留相同 Postgres、NATS volumes、template 和 provider 配置。

### 操作

1. 发送明确要求调用一次 `echo` 的合成消息。
2. 等待 pending approval 出现，记录 AgentRuntimeId、AgentId、SessionId、TurnId、ApprovalId 与当前 high-water；不要 resolve。
3. 重启 API 服务并等待重启后的 API process ready；记录 Compose 是复用原 container 还是重新创建，不把 container ID 必须变化当作通过条件。
4. 硬刷新 Web 页面，从左侧 recent conversation 列表显式重新选择并进入同一 AgentRuntime。
5. 确认同一 ApprovalId 从 Postgres view 恢复，并显示 Turn 需要显式 resume。
6. 在 unhosted 状态下 approve；确认决定已持久化，但 Turn 没有自动 resume。
7. 点击显式 Resume，等待原 Turn 继续并到达 durable terminal。

### 预期

- API 重启后 runtime 保持同一 AgentRuntimeId、pinned AgentId、SessionId 与 TurnId。
- pending approval 由 Postgres ledger 恢复；NATS 没有旧 frame 也不影响页面收敛。
- resolve 与 resume 是两个独立动作：resolve 成功后仍为 running/unhosted，只有显式 Resume 才重新 hosting。
- resume 不追加第二个 `LoopStarted`，不重复 `ToolApprovalRequested`，不重新询问已 resolve 的决定。
- 最终仅有一个 `ToolApprovalResolved`、一个 Tool execution 和一个 terminal，event sequence 连续。

## J06 — Pending cancel 等待 durable terminal

### 独立前置

- 独立确认 stack health。
- 在 Web 打开新的空白对话；首条长响应消息将创建本 journey 的 fresh AgentRuntime。

### 操作

1. 发送一条会产生足够长流式响应的合成消息。
2. 在 exact Turn 仍为 hosted/running 且正在输出时点击 Cancel。
3. 只在观察到 cancel command 返回 `202` 后继续本次 journey；若 Turn 已提前结束，放弃该尝试并用另一个 fresh AgentRuntime 重试。
4. 立即记录 Postgres status 与 Web 状态，然后等待 durable terminal。
5. terminal 后硬刷新页面，从 recent conversation 列表显式重选原 AgentRuntime，再次读取 AgentRuntimeView。

### 预期

- `202` 只让 Web 显示“取消请求已发送”一类 pending/advisory 状态；PG 在 terminal commit 前仍为 `running`。
- Web 不会因 command acknowledgement 提前伪造 `cancelled`。
- 最终允许 `LoopCancelled` 或竞争获胜的 `LoopFinished`，但只能存在一个 terminal。
- terminal 到达后 cancel pending 被清理，显式重选后的页面与 Postgres status 一致。
- 本 journey 不重启 API，也不声称 cancel intent 可跨进程恢复。

## J07 — 超过一页的 history

### 独立前置

- 独立确认 stack health。
- 在 Web 打开新的空白对话；首条普通消息将创建本 journey 的 fresh AgentRuntime。
- 使用短小、合成、低成本的普通 LLM 消息。

### 操作

1. 在同一 runtime 顺序完成足够多的短 Turn，直到最新 history 响应明确 `has_more=true`。
2. 记录初始 history 的 `through_event_seq`、当前最早可见 item 与页面消息数量。
3. 在 Web 中向上滚动，触发加载至少一页更旧 history。
4. 继续滚动直到确认跨越首个 page boundary，记录每页 cursor 与安全 event sequence。

### 预期

- Web 初始只加载最新窗口；只有向上滚动时才请求更旧 history。
- 更旧 item 按 event sequence 顺序插入，不重复、不反转，也不改变当前 AgentRuntimeView/status。
- 分页期间使用同一 fixed `through_event_seq` 与单调向后的 exclusive cursor。
- 页面加载超过一页后仍无 ghost draft、重复 terminal 或跨 runtime message。
- 原始 conversation event 永久可读；本 journey 不要求或伪造 compaction marker。

## J08 — 同 AgentId 多 AgentRuntime 隔离

### 独立前置

- 独立确认 stack health。
- 在两个浏览器 tab/context 中分别打开 Web 新对话并选择同一 template；不要预先调用 create API。

### 操作

1. 在两个新对话各发送一条不同的短合成消息，使 Web 以独立创建 intent/key 建立两个 fresh AgentRuntime；从两个页面的 Network/API 记录各自 create response 的 AgentRuntimeId、AgentId、template name/version。
2. 确认两个 runtime pin 同一 AgentId，但 AgentRuntimeId 不同。
3. 允许两个首 Turn 并发运行并分别等待 durable terminal。
4. 分别观察 SSE、最终 view、history 与安全 ledger sequence。
5. 在两个页面之间切换并各自硬刷新一次；每次刷新后都从 recent conversation 列表显式重新选择该页面原本的 AgentRuntime。

### 预期

- 两个 runtime 共享同一 immutable AgentId/template version，但拥有不同 AgentRuntimeId。
- 两边第一次 durable event 都从各自的 `event_seq=1` 开始，sequence 与 status 独立。
- 任一页面只应用同时匹配自身 AgentRuntimeId 与 pinned AgentId 的 frame/history。
- 两边的 Session、Turn、draft、approval、terminal、history 和 cursor 不串线。
- 刷新后仍恢复各自的 Postgres truth，不因共享 AgentId 合并成同一 conversation。

## 中央结果表

失败必须关联 issue 并在修复后以 fresh AgentRuntime 重跑。不得用“基本符合预期”覆盖 identity 串线、event gap、重复 terminal、ghost draft 或 Postgres/Web 不一致。

| ID | Commit | AgentRuntimeId | 结果（PASS/FAIL/BLOCKED） | Evidence | Issue / 备注 |
|---|---|---|---|---|---|
| J01 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M01 |
| J02 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M02 |
| J03 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M03 |
| J04-A | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M04 approve |
| J04-R | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M04 reject |
| J05 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M05 |
| J06 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M06 |
| J07 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M07 |
| J08-A | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M08 runtime A |
| J08-B | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | 对应原 M08 runtime B |
| F01 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | NATS stop/recover |
| F02 | 本次提交工作树 | 未提交 | PASS | 本任务中的人工确认 | Postgres stop/recover |

## 本清单结束条件

只有以下条件同时成立，本文档所代表的当前 Alpha 人工端到端验收才算完成：

1. J01—J08 与 F01—F02 全部在同一 PR head 对应的工作树上 PASS；F01—F02 的步骤与详细证据保存在 `ALPHA_FAULT_INJECTION.md`，本表只汇总结果。
2. 每条 journey 使用自己的 fresh AgentRuntime；J04 approve/reject 与 J08 两个 runtime 的证据分别可追踪。
3. J02 证明普通 LLM Turn 从 Web command、realtime 到 durable terminal 完整收敛。
4. J03 证明硬刷新从 Postgres 恢复且没有重复 message 或 ghost draft。
5. J04-A 与 J04-R 分别证明 Echo approve 和 reject 路径。
6. J05 证明 pending approval 跨 API 重启恢复，resolve 不隐式 resume，显式 resume 沿用原 identity。
7. J06 证明 cancel `202` 不伪造 terminal，最终只接受一个 durable outcome。
8. J07 证明真实 Web 可以按需加载超过一页的 history。
9. J08 证明同 AgentId 下两个 AgentRuntime 的 ledger、realtime 与 Web conversation 相互隔离。
10. 所有 FAIL 都有已修复 issue，并在 fresh AgentRuntime 上重新验证为 PASS；BLOCKED 不得计作通过。
11. 相关 Rust、real Postgres/NATS/API integration、Web test/typecheck/lint/build 与 OpenSpec strict validation 仍保持通过。
12. 证据满足脱敏规则，未提交 provider key、本地 secret 配置或用户/Tool 敏感正文。

本文档的 J01—J08 只对应当前 production-like 人工 journey；F01—F02 是否完成，以 `ALPHA_FAULT_INJECTION.md` 为准，本文件中央表只汇总结果。P4a 精确故障测试基建（含确定性 API SIGTERM/drain/restart）、H5b production compaction 策略/Hook 与 H5c 产品/故障验收仍以 `CONTEXT.html` 工程待办为准。在当前独立 gate 满足前，不得仅凭 J01—J08 把 OpenSpec `10.4` 或 `10.12` 标记完成。
