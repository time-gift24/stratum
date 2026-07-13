# wyse-ontology-api

## HTTP 错误边界

- `IntoResponse` 是 Ontology HTTP 失败真正被处理的边界；5xx 错误只在这里记录一次，library 不安装全局 tracing subscriber。
- 对外响应保持不透明，不暴露内部错误链。
- 内部 tracing 使用安全的结构化字段描述 HTTP 状态、错误类别、错误代码、资源种类和资源 ID；已知安全的 typed error 可以记录 source chain，但 `Repository(Box<dyn Error>)` 只能记录受控类别/代码，绝不格式化任意 source。
- 不得记录 Object values、schema JSON、请求体、数据库 URL、凭据、token 或其他敏感用户数据。
