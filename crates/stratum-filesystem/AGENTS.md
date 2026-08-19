# stratum-filesystem 约定

## 职责范围

`stratum-filesystem` 负责 Agent 可见的虚拟文件系统 trait、虚拟路径校验和本地沙箱后端。其保留的职责仅限业务文件操作：`read`/`list`/`write`/`create`/`remove`/`apply-patch`。执行持久化（CAS、`record`、`get`/`put`、Agent 状态/历史/事件日志）与 authoring catalog 均不得重新引入——execution 与 Studio PostgreSQL 分别是唯一的执行和编排存储。

## 设计规则

- 公开文件 API 接收 `VirtualPath`，而不是原始字符串或宿主机路径。
- 路径必须保持为虚拟绝对路径，例如 `/README.md`。
- 错误或 `tracing` 中不得暴露宿主机路径、沙箱根目录或文件内容。
- 后端实现只应实现最小的文件原语。
- `Filesystem` trait 必须保持对象安全，以便运行时工具能够接收显式的 `Arc<dyn Filesystem>` 依赖。
- `apply-patch` 支持是一项具体的共享文件系统能力；它必须使所有路径保持虚拟路径，并使所有读写都通过 `Filesystem` trait 完成。
- `apply-patch` 的错误和输出不得暴露宿主机路径或文件内容。
- `remove_dir` 只能删除空目录。
- `write_file` 必须保持崩溃一致性：先写入同级临时文件，再以原子方式重命名；`list_dir` 必须隐藏尚在写入过程中的临时文件。
- 在有具体调用方需要之前，不得添加挂载路由器、注册表、工厂、管理器、只读策略、流式 I/O、通配模式匹配/搜索、监视、快照、远程后端或对象存储。
- 本地沙箱操作默认必须拒绝通过符号链接逃逸。
