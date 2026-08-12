# Alpha 外部故障注入

## 目的与范围

本文只记录当前 stock Compose 环境无需生产 failpoint 即可执行的两个 Alpha 外部故障。仓库默认使用 `podman compose`；使用 Docker 时，把下文命令中的 `podman` 替换为 `docker`，其余参数保持不变。

| ID  | 唯一故障            | Compose project             |
| --- | ------------------- | --------------------------- |
| F01 | 停止并恢复 NATS     | `stratum-alpha-fi-nats`     |
| F02 | 停止并恢复 Postgres | `stratum-alpha-fi-postgres` |

两例必须顺序执行并各自使用新的 Compose project、数据库 volume、NATS volume、AgentRuntime 和合成测试数据。每例完成后先保存证据，再执行该例的 cleanup；不得复用已经受过故障影响的 fixture。

本文不是完整故障注入计划，也不是协议定义。以下场景明确不在本轮执行范围内：

- 数据库 COMMIT acknowledgement 不确定；
- Tool 外部副作用前后崩溃；
- hosted Turn 精确暂停后的 API SIGTERM、drain 与重启；
- NATS slow/full、retention expiry、cursor expiry、dispatcher/SSE buffer overflow；
- 直接 SQL corruption、非法持久化 shape 或 identity 注入；
- 真实 compaction producer、阈值策略、摘要 Hook 与 producer crash window。

前五类由 `CONTEXT.html` 工程待办中的 P4a 后续测试基础设施负责；其中 API SIGTERM 必须等 scripted LLM/可观察 Tool 能稳定制造 hosted slow/pending Turn、process controller 能锁定精确进程与边界后再执行，不能继续依赖真实 provider 的偶然时序。production compaction 策略/Hook 由 H5b 设计，producer 与 consumer 的产品/故障验收由 H5c 负责。本文件不得据此把这些场景记为通过，也不得为执行它们增加公开 debug endpoint、生产 failpoint、特殊协议字段或第二套状态。

执行任一 fixture 前，必须先按 [ALPHA_TEST.md](ALPHA_TEST.md) 的“本地 provider secret 配置”生成 Git 忽略的 `.stratum/alpha/config.toml` 与 Compose override。下文每条 Compose 命令都显式传入该 override；缺失文件或空 credential 时不得开始故障注入。

## 共同安全边界

1. 只允许使用本文给出的两个精确 Compose project name。执行 `down -v` 前必须先用同一 project name 运行 `podman compose ... ps`，确认目标是本例 fixture。
2. 同一时刻只启动一个 fixture，避免两个 project 争用 `5173`、`18080`、`4222` 和 `8222` 端口。
3. 使用合成 prompt、低权限限额测试 provider key 和可丢弃资源。真实 key 只从安全环境变量生成前述本地未跟踪配置，不写入本文、日志或 Git；API 不会直接读取 `DEEPSEEK_API_KEY`。
4. 故障注入前必须记录 Git commit、容器 ID、AgentRuntimeId、AgentId、SessionId、TurnId、当前状态和 `last_event_seq`。
5. 证据不得包含 prompt、message content、summary、Tool arguments/result、provider body、API key、token、连接字符串或其他凭据。
6. 每例只施加标题所述的一个故障。不得同时重启其他服务、修改网络、改变 retention、暂停进程、执行 SQL mutation 或修改生产配置。
7. 每例使用新的浏览器 profile/context，开始时不得带入其他 Compose project 的 recent conversation/localStorage；例内保留该 context，确保 refresh 后仍能从 recent 列表重选本例 runtime。cleanup 后关闭该 context。

安全 durable 证据只查询 identity、状态、版本、事件类型和序号：

```sql
SELECT id AS agent_runtime_id, agent_id, status, session_id,
       current_turn_id, last_event_seq
FROM agent_states
WHERE id = '<agent_runtime_id>';

SELECT event_seq, turn_id, event_type, event_version
FROM durable_events
WHERE agent_runtime_id = '<agent_runtime_id>'
ORDER BY event_seq;
```

每次查询还要确认 `event_seq` 从 `1` 到 `last_event_seq` 连续。HTTP 证据记录 status 和稳定字段或 `error.code`，不保存敏感正文。

## F01 — NATS stop / recover

### 独立 fixture

```sh
podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml up -d --build
podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml ps
```

等待 Postgres、NATS、API 和 Web 全部健康。在 Web 创建只属于本例的 AgentRuntime，完成一个安全基线 Turn，并记录其 identity、AgentRuntimeView barrier 和 durable event type 清单。保持该对话页面打开。

### 唯一故障

只停止 `nats` 服务：

```sh
podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml stop nats
```

不得同时停止或重启 Postgres、API、Web，不得修改 NATS retention 或人为填满队列。

### 操作

1. 确认 NATS 已停止，而 Postgres、API 和 Web 容器仍在运行。
2. 请求 `GET /health/live` 和 `GET /health/ready`。liveness 应为 `200`；readiness 仍应为 `200`，但其 `realtime` 是“最近一次 broker 操作”的状态，故障刚发生而尚无新操作时允许暂时仍为 `"ok"`。
3. 对该 runtime 发起一次新的 SSE reconnect/cold bootstrap，使服务实际尝试 broker subscription；它应返回稳定的 `503 realtime_unavailable`。随后轮询 readiness，直到报告 `realtime: "degraded"`。
4. 在同一 AgentRuntime 提交一条新的安全消息，记录 command HTTP status、SessionId 和 TurnId。不要把 SSE 建立失败误判成 command 失败。
5. 通过 AgentRuntimeView、固定 barrier 的 history 和安全 SQL 查询观察该 Turn；必要时硬刷新页面，再从 recent conversation 列表显式重选原 AgentRuntime以触发 PG cold bootstrap/reconcile。
6. 恢复 NATS 并等待容器健康：

   ```sh
   podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml start nats
   podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml ps
   ```

7. readiness 不会主动探测 NATS。先提交一个新的安全 Turn，让 PG commit 后的 publish 成为恢复探针；等待一次成功 publish 后再确认 readiness 的 `realtime: "ok"`，重新建立页面实时连接，并验证该 Turn 最终从 PG 收敛且后续 tail 可用。

### Durable oracle

- NATS 停止不改变 Postgres 的唯一真相；PG-backed command 不因 realtime 故障回滚。
- 故障期间成功提交的 durable rows 与 `agent_states.last_event_seq` 一致，序号连续且无重复。
- NATS publish 或连接失败不得使同一 kernel fact再次 append，也不得伪造 terminal。
- 恢复后页面缺失的 durable product 以 AgentRuntimeView/history 为准收敛；不得要求 NATS 承担 durable backlog。

### 用户可见 oracle

- 页面明确表现为实时连接降级或重连，不把它展示成执行存储失败。
- 已接受的消息最终由 PG 状态收敛为完整 assistant message/terminal；允许故障期间 delta 不完整或丢失。
- refresh/reconnect 后没有重复消息、ghost draft 或错误 runtime；恢复后的新 Turn 可以继续收到 realtime tail。

### 证据

- 故障前、停止后、恢复后的 `podman compose ... ps` 与容器 ID；
- 三个时点的 live/ready HTTP status 和 readiness 安全字段；
- command status、AgentRuntime/Session/Turn identity；
- 故障前后安全 SQL 查询、barrier 和连续 event type 清单；
- 降级、PG 收敛以及 realtime 恢复后的页面截图；
- 只含安全 metadata 的 API/NATS 日志片段，不保存 frame payload。

### Cleanup

```sh
podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml ps
podman compose -p stratum-alpha-fi-nats -f docker-compose.yml -f .stratum/alpha/compose.override.yml down -v --remove-orphans
```

确认该 project 的容器、network 和两个数据 volume 已清理，再开始 F02。

## F02 — Postgres stop / recover

### 独立 fixture

```sh
podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml up -d --build
podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml ps
```

等待全部服务健康。在 Web 创建本例专用 AgentRuntime并完成一个安全基线 Turn；记录 AgentRuntimeView、固定 barrier history 和安全 SQL 基线。

### 唯一故障

只停止 `postgres` 服务：

```sh
podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml stop postgres
```

不得同时停止 NATS、API、Web，不得删除 volume、修改 SQL、改变连接配置或重启 API。

### 操作

1. 确认 Postgres 已停止，NATS、API 和 Web 容器仍在运行。
2. 请求 `GET /health/live` 与 `GET /health/ready`。API 进程仍存活时 liveness 应为 `200`；readiness 应为 `503` 且 `status: "unavailable"`。
3. 分别请求该 AgentRuntime 的 view、history 和一条 command。记录稳定的 `503 store_unavailable`，不得从 NATS 或页面缓存猜测成功状态。
4. 恢复 Postgres并等待健康：

   ```sh
   podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml start postgres
   podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml exec -T postgres pg_isready -U stratum -d stratum
   ```

5. 等待连接池恢复，重新请求 readiness、view 和原 fixed-barrier history；硬刷新 Web 后从 recent conversation 列表显式重选原 AgentRuntime，确认页面从 PG 恢复。
6. 使用当前 exact Turn CAS 重新提交一条安全消息；把它视为新的明确操作，不假定故障期间的失败 command 已提交。
7. 读取安全 SQL oracle，比较故障前后 high-water 和 event type 序列。

### Durable oracle

- Postgres 不可用时所有执行真相入口 fail closed；NATS、Web cache 或内存 registry不得替代 durable truth。
- 故障期间返回 `store_unavailable` 的操作不得留下被系统猜测的成功、terminal 或 event sequence 空洞。
- Postgres 恢复后原 AgentRuntime、pinned Agent、Session 和 Turn identity不变，故障前已提交的 rows 完整存在。
- 恢复后的下一次成功 append 使用连续的 next `event_seq`；没有重复 row 或已推进但缺 row 的 high-water。

### 用户可见 oracle

- 页面显示存储暂不可用或请求失败，不把缓存状态冒充成最新真相，也不伪造 finished/failed/cancelled。
- 存储错误只出现在 conversation 外的单一、可操作 notice 中，不作为 assistant/system 正文插入 timeline；故障前后的历史消息条数保持不变。
- 恢复并 refresh/reconcile 后，同一 AgentRuntime 的既有历史重新出现，不创建替代对话。
- Postgres 恢复且一次正常 view 或明确重试的 Turn 成功后，notice 丝滑退出且只退出一次；不得抢走当前焦点、强制滚动或遮挡已恢复历史。
- 用户明确重试后可以继续运行；失败期间的输入不得以 ghost message 形式自行出现。

### 证据

- 故障前、停止后、恢复后的 `podman compose ... ps` 与容器 ID；
- live/ready status 和 readiness 安全字段；
- view/history/command 的 HTTP status 与 `error.code`；
- 故障前后安全 SQL 查询和 event sequence 连续性；
- 存储不可用、恢复后同一对话和明确重试结果的页面截图；
- 记录 notice 所在 DOM 区域、恢复前后消息数量、焦点目标与滚动位置，确认错误未进入对话正文且退出不扰动阅读位置；
- 只含安全 metadata 的 API/Postgres 日志片段。

### Cleanup

```sh
podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml ps
podman compose -p stratum-alpha-fi-postgres -f docker-compose.yml -f .stratum/alpha/compose.override.yml down -v --remove-orphans
```

确认该 project 已完全清理，本轮 stock Compose 外部故障即执行完毕。API SIGTERM/drain/restart 的确定性场景由 `CONTEXT.html` 工程待办 P4a 接管。

## 本轮完成判定

每例分别记录 `PASS`、`FAIL` 或 `BLOCKED`，不能用另一例的证据补齐。只有以下条件同时满足时，本文件覆盖的外部故障注入才算完成：

1. F01 证明 NATS 故障只降级 realtime，durable truth最终从 PG 收敛；
2. F02 证明 Postgres 故障使核心读写 fail closed，恢复后 sequence 与 identity连续；
3. 两例均使用独立 fixture、单一故障并完成 cleanup；
4. 所有证据满足本文的脱敏边界。

执行结果还必须回填 `ALPHA_TEST.md` 的中央结果表。该结论不覆盖 P4a、H5b 或 H5c 延期的任何场景。
