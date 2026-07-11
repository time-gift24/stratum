# wyse-ontology-mysql

## 范围

- 本 crate 仅实现 `wyse-ontology::OntologyRepository` 的 MySQL 8/SQLx 后端。
- `SqlxOntologyRepository` 只接收由外部构造的 `MySqlPool`；不拥有应用启动、连接池配置或服务组合。

## 持久化约定

- migration 只位于 `migrations/`，必须由 SQLx CLI 手动执行；运行时不得迁移或修改数据库 schema。
- Object 与 Link 是跨所有 schema reference 共享的数据。对象删除默认依赖外键拒绝；`force` 删除在同一事务中先删关联 Link 再做带版本的 Object 删除。
- Object/Link 的写入使用乐观锁。受影响行数为零时必须区分资源缺失与版本冲突。
- `publish_revision`、`move_online_tag` 与全部 Object/Link 写入共用固定 MySQL named advisory lock；获取和释放必须发生在同一池连接。`publish_revision` 与 `move_online_tag` 必须在持锁的同一 transaction 内加载 Object 和 Link、校验 schema，再分别写入 revision 或更新 `online` tag。持锁 future 被取消时必须在 Drop 中 `close_on_drop`，释放失败也不得把可能持锁的 session 归还连接池。
- 强制删除 Object 时，事务必须先按 Link 写入相同的对象行锁协议锁定该 Object，再删除关联 Link 与带版本的 Object。
- 只使用 `sqlx::query()` 和 bind 参数，不使用编译期数据库连接的 `query!` 宏。

## 测试

- MySQL 集成测试默认 `#[ignore]`；用 `make test-integration` 启动 MySQL 8、执行 CLI migration、运行测试，并清理 volume。
