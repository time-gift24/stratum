# stratum-config 约定

- 配置、agent template 和 resolved definition 都使用严格 schema；未知字段必须拒绝，不能静默忽略拼写错误。
- 默认模型和 template 选择的模型必须属于对应已配置 provider 的 `models` 列表；provider 缺失或模型未登记均为配置错误。
- `ResolvedAgentDefinition` 只保存运行所需的名称、模型、工具和 prompt，不得包含 API key、token 或其他 provider secret；凭据只保留在 provider 配置与构造边界。
- `[api]` 存在但省略 `bind` 时固定默认 `127.0.0.1:8080`；`allowed_origins` 在配置解析边界校验为合法 `http::HeaderValue` 并拒绝 `*`，router 不得静默过滤。shutdown drain、SSE keepalive、approval fallback poll 与 dispatcher idle timeout 均为正秒数配置，零值 fail closed。
- 执行存储配置收敛为 `[agent].templates_root` 与 `[postgres].url`：没有 backend selector 或静默回退。`templates_root` 指向只读 template catalog；本 crate 只校验非空，路径存在、是目录且可读的启动校验由装配层 `stratum-api` 完成（空目录允许，服务绝不自动创建目录）。`postgres.url` 缺失（`require_postgres`）或为空即失败（fail closed）。
- NATS 连接 timeout 与 Agent 短 tail 的 age/bytes/message-count 上限继续放在 `[nats]` 能力配置中，解析边界完成字段校验（非空字符串、正数 timeout/上限、replicas 1..=5）；不编码固定历史保留保证。config → infra runtime 类型的映射由 `stratum-api` 装配时完成，本 crate 不依赖 `stratum-infra`。
- `ProviderConfig` 支持可选 `base_url` 覆盖（缺省时由装配层使用 provider 官方公开端点常量）；空串在解析边界拒绝。connect、non-stream request、first response 与 stream chunk idle timeout 均为正秒数并真实传入 provider。`api_key` 的 `Debug` 输出必须为 `[redacted]`，新增凭据字段同样不得进入 `Debug`/`Display`。
- `ProviderConfig.api_key` 以 `secrecy::SecretString` 承载（§6）；装配层通过 secret wrapper 的所有权转换交给 `ApiKey`，不得先 `expose_secret()` 生成普通 `String` 副本。
