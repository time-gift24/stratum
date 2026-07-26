# Stratum 技术架构

> 文档状态：架构总览草案
>
> 基线版本：`63c1e001367b18ecba91d8dd02a4c2d2fc903e7b`

## 总体架构

```mermaid
flowchart TB
    User["最终用户"] --> Web["Stratum Web"]
    Developer["开发者 / 运营人员"] --> Web
    External["外部系统"] --> API["Web API / SSE"]
    Web --> API

    subgraph Control["接入与控制面"]
        API --> Definition["Agent / Workflow / Skill / Extension 定义"]
        API --> Governance["身份、权限、版本与运行控制"]
        Registry["模型 / 工具 / 扩展注册"] --> Definition
    end

    subgraph Runtime["执行面"]
        Workflow["工作流引擎<br/>图校验 · 队列调度 · 节点状态"]
        AgentNode["Agent 节点"]
        Agent["Agent ReAct Loop<br/>模型调用 · 工具调用 · Turn 控制"]
        Hook["Hook 运行时<br/>上下文 · Tool 前后 · 下一轮"]

        Workflow --> AgentNode --> Agent --> Hook
    end

    Definition --> Workflow
    Governance --> Workflow

    subgraph DIY["Agent DIY"]
        Skill["第一档：Skill"]
        Script["第二档：Deno / Python Hook"]
        Rust["第三档：Rust Hook / Hook Service"]
    end

    Skill --> Hook
    Script --> Hook
    Rust --> Hook

    subgraph Capability["运行能力"]
        LLM["LLM Provider"]
        Tool["Tool Registry / Tool"]
        Workspace["Agent 虚拟工作区"]
    end

    Agent --> LLM
    Agent --> Tool
    Tool --> Workspace

    subgraph Data["数据与事件"]
        Session["长期 Session<br/>图无关 · 多 Agent"]
        State["运行状态与执行日志"]
        History["Agent 对话历史"]
        Artifact["Skill / Extension / Workflow 制品"]
        EventBus["事件流与重放"]
    end

    Session --> State
    Workflow --> Session
    Agent --> History
    Hook --> State
    Definition --> Artifact
    Workflow --> EventBus
    Agent --> EventBus

    EventBus --> Observe["Web 实时呈现 / 审计 / 可观测性"]
```

## 架构说明

### 1. Web 接入与控制面

`stratum-web` 和 `stratum-api` 对外提供对话、工作流编辑、运行控制、审批、历史查询和 SSE 事件流。

控制面管理 Session 以及 Agent、Workflow、Skill 和 Extension 的定义与发布版本，并负责身份、权限、Secret 引用和能力注册。Agent 可以在没有活跃操作时修改当前模型及其 LLM 参数；新 Turn 接受后会把该 `ModelConfig` 保存为 Agent 后续默认值。一个 Turn 开始后，其 Agent、Skill 集、Extension 集、Hook Handler 顺序、模型配置和工具集指纹保持不变，恢复也必须使用原 Turn 固定的配置。

Web/API 只负责接入和组合，不实现工作流调度与 Agent 循环。

### 2. 工作流引擎

工作流引擎负责节点级编排：

- 编译和校验工作流图；
- 维护变量池与节点依赖；
- 使用有界队列调度就绪节点；
- 持久化 Workflow 与节点状态；
- 支持暂停、恢复、取消和人工输入；
- 发布 Workflow 与节点事件。

Agent 可以作为工作流中的一种节点，也可以直接在 Session 中运行。直接对话不是隐式工作流：Session 是长期、共享且与图结构无关的核心资产，Workflow 图及其版本可以变化，Session 身份保持不变。Agent 作为节点运行时使用 `AgentLocation::WorkflowNode` 保留 Workflow version 与 node 身份；直接运行时使用 `AgentLocation::Direct`。

一期只允许每个 Session 同时存在一个活跃操作。这是一阶段的简化设计，不是永久产品限制；在真正引入并发前不增加 attempt 或 node-execution 身份。

### 3. Agent ReAct Loop 与 Hook

Agent 内核只负责稳定的执行机制：

```text
构造模型请求
→ 消费模型响应
→ 提交 Assistant 消息
→ 执行 Tool Call
→ 提交 Tool Result
→ 继续或结束 Turn
```

策略通过四个核心 Hook 进入循环：

- `transform_context`
- `before_tool_call`
- `after_tool_call`
- `prepare_next_turn`

多个 Hook Handler 按固定版本和顺序执行。会影响 Resume 的参数修改、阻断、结果变换和下一轮决策必须写入独立的 Session/Turn Hook journal，恢复时按语义 invocation identity 复用。该 journal 与 Agent 对话历史、EventBus 观察流分离，存储实现推迟到 H3/P1。

Hook、Registry、Event 和 Port 不混用：Hook 修改当前流程，Registry 注册能力，Event 只用于观察，Port 用于替换外部实现。

### 4. 三档 Agent DIY

| 档位 | 用户提供的内容 | 运行方式 |
|---|---|---|
| 第一档 | Skill 指令和资源 | 通用 Agent 在上下文构造阶段加载 |
| 第二档 | Deno 或 Python Hook | 独立 Extension Host 进程执行 |
| 第三档 | Rust Hook | 受信任的链接式 Hook 或独立 Hook Service |

三档能力共享不可变版本、权限、审计和 Turn runtime snapshot 固定机制。Skill 不执行任意代码；Deno/Python 默认与 Agent 进程隔离；Rust 动态共享库不作为通用扩展机制。

### 5. 文件系统与存储

文件与存储分成三个逻辑域：

| 数据域 | Agent 是否可见 | 主要约束 |
|---|---:|---|
| Agent 工作区 | 是，通过 `VirtualPath` | 沙箱、权限、容量和路径安全 |
| 运行时持久化 | 否 | CAS、一致性、幂等和恢复 |
| 制品存储 | 否 | 不可变版本、内容摘要和分发 |

本地开发时三者可以使用同一物理文件系统，但接口、命名空间和 ACL 保持隔离。EventBus 提供实时事件和有限重放，不能替代运行状态、Agent 历史和 Hook 决策的持久化存储。

### 6. 可观测性与安全

当前统一运行层级为：

```text
Session → 可选 Workflow Node → Agent Turn → LLM / Tool / Hook
```

每层使用结构化 tracing、metrics 和类型化事件。默认不记录 Prompt、Tool 参数、Tool Result、Secret 或主机路径。

所有定义、制品、Session 和扩展服务带有租户与项目作用域；不受信任的 Script Hook 运行在受限进程或容器中，并限制网络、文件、时间、内存和输出。

## 代码模块映射

| 架构职责 | 模块 |
|---|---|
| 公共身份与事件类型 | `stratum-core` |
| Agent ReAct Loop | `stratum-agent` |
| 通用 Agent | `stratum-agent-builtin` |
| Tool | `stratum-tools` |
| LLM Provider | `stratum-llm` |
| Agent 虚拟工作区 | `stratum-filesystem` |
| Agent 状态与历史 | `stratum-store` |
| 事件基础设施 | `stratum-infra` |
| HTTP API 与运行时组合 | `stratum-api` |
| Web 产品 | `stratum-web` |
| 工作流引擎 | `stratum-workflow` 逻辑模块 |
| Hook 运行时 | `stratum-hook` 逻辑模块 |
| Deno/Python 扩展宿主 | `stratum-extension-host` 逻辑模块 |

逻辑模块不要求与 Cargo crate 一一对应；只有形成独立职责和依赖边界时才拆分 crate。
