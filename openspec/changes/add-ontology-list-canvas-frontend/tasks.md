# Tasks: add-ontology-list-canvas-frontend

## 1. 产品文档

- [x] 1.1 按 impeccable 流程更新 PRODUCT.md：产品范围从单对话页扩展为「对话 + 白板 + Ontology 管理」，措辞保持「前端按契约实现，后端联调待落地」
- [x] 1.2 更新 stratum-web/DESIGN.md：补充 Ontology 列表与画布的组件规范，明确不复用 legacy canvas tokens、xyflow 仅经 CSS variables 主题化

## 2. 基础层

- [x] 2.1 新增依赖 `@xyflow/react`（pnpm），确认样式经 CSS variables 接入语义 token
- [x] 2.2 `features/ontology-editor/types.ts`：Ontology 资源文档 DTO（object_types / link_types / properties / canvas.positions）、错误 envelope 与 violations 类型，与 `docs/ontology/API.md` 对齐
- [x] 2.3 扩展 `lib/stratum/api.ts`：ontology 方法组（list / create / get / replace / delete / neighborhood），ETag 头读写，`ApiError` 扩展可选 `violations`
- [x] 2.4 UUIDv7 生成器（`crypto.getRandomValues`，纯函数）+ 单测
- [x] 2.5 RFC 6901 JSON Pointer 解析器（`~0`/`~1` 反转义，纯函数）+ 单测
- [x] 2.6 `features/ontology-editor/recovery.ts`：IndexedDB 原生薄封装，单表单条 `{ ontology_id, base_etag, candidate }`

## 3. 编辑器状态机

- [x] 3.1 `features/ontology-editor/reducer.ts`：acknowledged / candidate / in_flight 状态与编辑 action（增删改 object type / property / link type / 拖拽位置）
- [x] 3.2 reducer 保存生命周期：`saveStarted` / `saveSucceeded`（仅确认 in_flight 快照）/ `saveConflict`（412，candidate 不变）/ `saveInvalid`（422，candidate 不变）
- [x] 3.3 reducer 穷举单测：成功无新编辑 / 成功有新编辑 / 412 / 422 / 超时先读后判 / 草稿恢复与丢弃
- [x] 3.4 violations JSON Pointer → 节点/属性/边/全局 的映射函数 + 单测
- [x] 3.5 客户端校验与 MVP 上限（name 正则、500 object types、100 properties/type、10000 总 properties、2000 link types、500 positions）+ 单测
- [x] 3.6 `hooks/use-ontology-editor.ts`：reducer + API 副作用编排（加载、保存、调和、草稿持久化、超时重读）

## 4. 列表页

- [x] 4.1 `app/(site)/ontologies/page.tsx` + `components/stratum/ontology/` 列表组件：分页、排序 `-updated_at`、加载/错误/空态
- [x] 4.2 新建流程：表单（name 正则内联校验）、POST 成功后携响应文档与 ETag 跳转编辑器、409 名称冲突内联提示
- [x] 4.3 删除流程：确认对话框、先读 ETag 再 DELETE + If-Match、412 提示并刷新
- [x] 4.4 `components/chrome/site-chrome.tsx` 导航新增 Ontology 入口，验证对话与白板入口不受影响

## 5. 画布编辑器

- [x] 5.1 `app/(site)/ontologies/[id]/page.tsx`：加载资源、404 提示、错误重试；xyflow 受控渲染节点/边/positions
- [x] 5.2 无位置节点的确定性网格布局（按文档数组序）
- [x] 5.3 Object Type 节点编辑：新增 / 编辑（name、display_name、description、properties 含 value_type 枚举）/ 删除（引用它的 link type 一并移除并提示）
- [x] 5.4 Link Type 连线交互：源/目标 + source_to_target / target_to_source（one|many）标注渲染
- [x] 5.5 拖拽位置写入 candidate canvas.positions；删除节点同步移除其位置
- [x] 5.6 保存：PUT 整文档 + If-Match；成功更新 acknowledged 与新 ETag；飞行期间编辑保留 candidate
- [x] 5.7 412 调和对话框：重新读取最新资源，用户显式选择保留本地 / 采用远端，禁止静默重试
- [x] 5.8 422 violations 内联映射展示（节点/属性行 + 全局错误区），candidate 不变
- [x] 5.9 崩溃恢复：candidate 变化写 IndexedDB 草稿；加载时发现草稿提示恢复 / 丢弃；保存成功清除草稿；超时先 GET 判断 in_flight 是否已提交
- [x] 5.10 neighborhood 只读聚焦视图：depth 0–5、404 object_type_not_found 提示；编辑器内聚焦由本地 candidate 计算

## 6. 验证

- [x] 6.1 全部单测通过（reducer / pointer / uuid / recovery / 校验上限 / api client mock fetcher）
- [x] 6.2 `pnpm lint` 与类型检查通过；人工走查列表与画布交互（加载 / 空态 / 错误态 / 保存 / 412 / 422 / 恢复）
- [x] 6.3 对照 PRODUCT.md / DESIGN.md 走查视觉与文案（中文、语义 token、无装饰动画、prefers-reduced-motion）
- [x] 6.4 constitution-review：对照根 CONSTITUTION.md 派发子代理分条款审查本 change 完整 diff，修复全部 red-flag 与 violation
- [x] 6.5 更新 `stratum-web/AGENTS.md`（或对应 AGENTS.md）归档最终实现约定
- [x] 6.6 同步后端澄清：子实体 ID 在全部现存 Ontology 中按类型全局唯一，冲突返回 409 `ontology_entity_id_conflict`，硬删除无 tombstone——API.md、spec delta 与保存失败 code 文案映射已同步，并对完整新 diff 重新运行验证与 constitution-review

## 7. 归档准备

- [x] 7.1 运行 `openspec validate --all --strict` 通过，确认失效 change 不会合并到主 specs
- [ ] 7.2 确认所有任务真实完成并验证后，执行 `/opsx:archive`
