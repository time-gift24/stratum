# stratum-config 约定

- 配置、Agent 模板和解析定义都使用严格模式；未知字段必须拒绝，不能静默忽略拼写错误。
- 默认模型和模板选择的模型必须属于对应已配置提供方的 `models` 列表；提供方缺失或模型未登记均为配置错误。
- 模板目录必须校验作者提供的 `version` 字符串标签；标签大小写敏感、UTF-8 编码长度为 `1..=128` 字节、无控制字符和首尾空白，不做空白修剪、规范化、SemVer 解析或排序。规范解析定义只保存运行所需的模型、按序工具和提示词，不得包含 `name`/`version`、运行时覆盖项、API 密钥、令牌或其他提供方机密信息；凭据只保留在提供方配置与构造边界。
- `[api]` 存在但省略 `bind` 时固定默认 `127.0.0.1:8080`；`allowed_origins` 在配置解析边界校验为合法 `http::HeaderValue` 并拒绝 `*`，路由器不得静默过滤。关闭排空、SSE 保活与调度器空闲超时均为正秒数配置，零值必须从严失败；审批回退重读周期是内部固定上限，不得配置化。
- 执行存储配置收敛为 `[agent].templates_root` 与 `[postgres].url`：没有后端选择器或静默回退。`templates_root` 指向只读模板目录；本 crate 只校验非空，路径存在、是目录且可读的启动校验由装配层 `stratum-api` 完成（空目录允许，服务绝不自动创建目录）。`postgres.url` 缺失（`require_postgres`）或为空即从严失败。
- NATS 连接超时与 AgentRuntime 短尾流的保留时长/字节数/消息数上限继续放在 `[nats]` 能力配置中，解析边界完成字段校验（非空字符串、正数超时/上限、`replicas` 为 `1..=5`）；不编码固定历史保留保证。配置类型到基础设施运行时类型的映射由 `stratum-api` 装配时完成，本 crate 不依赖 `stratum-infra`。
- `ProviderConfig` 支持可选 `base_url` 覆盖（缺省时由装配层使用提供方官方公开端点常量）；空串在解析边界拒绝。连接建立、非流式请求、首个响应与流式数据块空闲超时均为正秒数并真实传入提供方。`api_key` 的 `Debug` 输出必须为 `[redacted]`，新增凭据字段同样不得进入 `Debug`/`Display`。
- `ProviderConfig.api_key` 以 `secrecy::SecretString` 承载（§6）；装配层通过机密值包装器的所有权转换交给 `ApiKey`，不得先 `expose_secret()` 生成普通 `String` 副本。
