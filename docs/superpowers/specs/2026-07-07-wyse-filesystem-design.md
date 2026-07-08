# Wyse Filesystem Crate 设计

日期：2026-07-07

## 目标

创建 `wyse-filesystem` crate，定义 agent 可见的虚拟文件系统边界，并提供第一个 local sandbox 实现。

本轮做：

- async `Filesystem` trait
- 虚拟绝对路径 `VirtualPath`
- 整文件读写，不做 stream
- 目录列举和基础 metadata
- 创建目录、删除文件、删除目录
- local sandbox backend

本轮不做多 mount、read-only/write policy、文件 watch、snapshot/versioning、glob/search、远程文件系统、对象存储或 agent patch 操作。

## 设计原则

agent 和 runtime 只看到虚拟路径，例如 `/README.md`、`/src/lib.rs`。真实宿主机路径只存在于 backend 内部，不进入公共 API、错误文本或 tracing 字段。

`Filesystem` trait 值得存在，因为本轮同时需要稳定的 agent-facing 边界和一个真实 local backend。除此之外不引入 registry、factory、manager、mount router 或 facade。

首版只支持整文件 `Bytes` 读写。大文件 stream、增量写入、文件句柄生命周期和背压语义都留到真实需求出现后再设计。

权限策略不放在本 crate 首版中。`wyse-filesystem` 只负责虚拟路径和 sandbox 安全；agent runtime 或 policy 层决定某个 agent 是否允许写入。

## 架构

`wyse-filesystem` 分为六个小模块：

- `src/definition.rs`：`Filesystem` trait、`FileMetadata`、`DirEntry`、`FileType`
- `src/path.rs`：`VirtualPath` newtype 和路径校验
- `src/error.rs`：`FilesystemError`
- `src/local.rs`：`LocalFilesystem` 和 `LocalFilesystemConfig`
- `src/lib.rs`：crate docs 和 public re-export

`Filesystem` 使用原生 async trait 方法：

```rust
#[allow(async_fn_in_trait)]
pub trait Filesystem: Send + Sync {
    async fn read_file(&self, path: &VirtualPath) -> Result<Bytes, FilesystemError>;

    async fn write_file(
        &self,
        path: &VirtualPath,
        contents: Bytes,
    ) -> Result<(), FilesystemError>;

    async fn list_dir(&self, path: &VirtualPath) -> Result<Vec<DirEntry>, FilesystemError>;

    async fn metadata(&self, path: &VirtualPath) -> Result<FileMetadata, FilesystemError>;

    async fn create_dir(&self, path: &VirtualPath) -> Result<(), FilesystemError>;

    async fn remove_file(&self, path: &VirtualPath) -> Result<(), FilesystemError>;

    async fn remove_dir(&self, path: &VirtualPath) -> Result<(), FilesystemError>;
}
```

`DirEntry` 返回虚拟路径、文件名、文件类型和可选 metadata。`list_dir` 的输出按虚拟路径字典序稳定排序，方便测试和 agent 消费。

`remove_dir` 只删除空目录。递归删除不进入首版，避免 agent 误删整棵目录树；等出现真实清理 workflow 后再单独设计。

## Path Model

`VirtualPath` 是公共 API 中唯一的路径类型。它只接受 `/...` 风格的虚拟绝对路径。

`VirtualPath::try_from` 必须拒绝：

- 空字符串
- 相对路径
- 没有 `/` 前缀的路径
- `..` segment
- 空 segment，例如 `/a//b`
- 反斜杠
- Windows drive prefix
- NUL 字节

根路径 `/` 合法，用于列举 sandbox root 或读取 root metadata。

`VirtualPath` 可以提供 `as_str()` 返回原始虚拟路径，也可以提供内部 helper 迭代已校验 segment。公共 API 不接受裸 `String` 或宿主机 `PathBuf`。

## Local Sandbox Backend

`LocalFilesystem` 持有一个真实 sandbox root，例如 `/tmp/wyse-run-123/workspace`。调用流程：

1. 接收 `&VirtualPath`
2. 去掉开头 `/`
3. 拼接到 sandbox root
4. 规范化并确认最终路径仍在 root 内
5. 调用 `tokio::fs`
6. 返回 Wyse 类型或 `FilesystemError`

local backend 在实际打开或写入文件前必须做越界检查。路径越界、非法路径、非 UTF-8 宿主路径和符号链接逃逸都返回类型化错误。

默认不允许符号链接逃逸 sandbox。如果路径中的符号链接指向 root 外，操作失败。以后如果有允许 symlink 的真实需求，再单独设计配置和风险边界。

`LocalFilesystemConfig` 首版只包含真实 root 和可选单文件大小限制。大小限制用于整文件读写，避免 agent 意外读取或写入过大的内容。

`apply_patch` 不属于 `wyse-filesystem` 首版。它是后续 agent/tool 层能力，届时应复用 `Filesystem` 的最小文件原语，而不是让 filesystem backend 各自实现 patch 语义。

## Error Handling

`FilesystemError` 使用 `thiserror`，错误消息小写开头、不加句号。错误类型至少覆盖：

- invalid virtual path
- path escapes sandbox
- not found
- already exists
- not a file
- not a directory
- directory not empty
- permission denied
- content too large
- local IO error

错误可以包含虚拟路径和安全 operation 名称。错误不能包含宿主机真实路径、sandbox root 或文件内容。

可失败的公共函数在文档中写 `# Errors`。

## Observability

library 只通过 `tracing` 发出事件或 spans，不安装全局 subscriber。

允许记录的安全字段：

- operation
- virtual path
- bytes length
- directory entry count

禁止记录：

- 文件内容
- 宿主机真实路径
- sandbox root
- secret 或 credential

错误只在真正处理它的边界记录一次。

## Testing

单元测试覆盖 `VirtualPath`：

- 合法路径
- 根路径
- 空路径
- 相对路径
- `..`
- 反斜杠
- 空 segment
- Windows drive prefix
- NUL 字节

local backend async 测试覆盖：

- sandbox 内读写列删
- root metadata
- 目录稳定排序
- 路径越界被拒绝
- symlink 逃逸被拒绝
- 单文件大小限制

常规验证：

- `cargo fmt`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets`

## Acceptance Criteria

- workspace 中有 `wyse-filesystem` crate
- crate 暴露 async `Filesystem` trait
- 公共文件 API 只接受 `VirtualPath`
- `VirtualPath` 拒绝相对路径、`..`、空 segment、反斜杠、Windows drive prefix 和 NUL 字节
- `LocalFilesystem` 把虚拟路径安全映射到 sandbox root
- local backend 默认拒绝 symlink 逃逸
- 支持整文件 `Bytes` read/write
- 支持 list、metadata、mkdir、remove file、remove empty dir
- 错误不泄露宿主机真实路径、sandbox root 或文件内容
- 普通 workspace 测试不依赖外部服务

## 后续可能扩展

以下能力不在首版实现，但可以在需求出现后自然接入：

- 多 mount router
- read-only/write policy
- 远程 filesystem backend
- 对象存储 backend
- stream read/write
- agent/tool 层 `apply_patch`
- glob/search
- file watch
- snapshot/versioning
- rename/move
