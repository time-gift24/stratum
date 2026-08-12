import { readFile, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { contextManual } from "./content/context.ts"
import {
  knowledgeStatuses,
  todoStatuses,
  type ConceptEntry,
  type EvidenceRef,
  type ManualChapter,
  type NarrativeCase,
  type RiskNote,
  type TodoInitiative,
  type TodoStatus,
} from "./content/model.ts"
import { todoLedger } from "./content/todo.ts"

const scriptDirectory = dirname(fileURLToPath(import.meta.url))
const webRoot = resolve(scriptDirectory, "..")
const repositoryRoot = resolve(webRoot, "..")
const outputArgumentIndex = process.argv.indexOf("--output")
const outputPath =
  outputArgumentIndex >= 0 && process.argv[outputArgumentIndex + 1]
    ? resolve(process.cwd(), process.argv[outputArgumentIndex + 1])
    : resolve(repositoryRoot, "CONTEXT.html")

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;")
}

const protectedTechnicalTokens = [
  "HookInvocationCompleted",
  "HookInvocationPending",
  "HookInvocationFailed",
  "ToolExecutionStarted",
  "TranscriptCompacted",
  "PrepareNextTurnDecision",
  "ExtensionSetVersionId",
  "WorkflowVersionId",
  "AgentRuntimeView",
  "AgentRuntimeId",
  "AgentRuntime",
  "TurnRuntimeSnapshot",
  "DurableAgentEvent",
  "IterationCompleted",
  "MessageAppended",
  "ReplaceSystemPrompt",
  "transform_tool_call",
  "prepare_next_turn",
  "transform_context",
  "decide_tool_call",
  "after_tool_call",
  "transcript_compactions",
  "committed context",
  "durable_events",
  "agent_states",
  "request view",
  "strict replay",
  "HookInvocationId",
  "HookAddress",
  "HookSnapshot",
  "ContextPatch",
  "ReplaceResult",
  "LoopFinished",
  "LoopStarted",
  "LoopOutcome",
  "DropHistory",
  "RewriteHistory",
  "AgentId",
  "SessionId",
  "TurnId",
  "CallId",
  "SkillSet",
  "SkillId",
  "Ready Queue",
  "Tool Call",
  "JoinSet",
  "event_seq",
  "last_event_seq",
  "durable fact",
  "Durable facts",
  "event stream",
  "COMMIT",
  "SIGTERM",
  "Postgres",
  "NATS",
  "SSE",
  "HTTP",
  "UUID",
  "JSON",
  "LLM",
  "Tool",
  "Hook",
  "Agent",
  "Workflow",
  "Node",
  "Wait",
  "Operation",
  "Effect",
  "Pending",
  "Completed",
  "Failed",
  "Continue",
  "Compact",
  "Inject",
  "Stop",
  "kernel",
  "journal",
  "provider",
  "runtime",
  "resume",
  "identity",
  "symbol",
] as const
const protectedTechnicalTokenSet = new Set<string>(protectedTechnicalTokens)
const protectedTechnicalPattern = new RegExp(
  `(${[...protectedTechnicalTokens]
    .sort((left, right) => right.length - left.length)
    .map((token) => token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("|")})`,
  "g"
)

function renderProse(value: string): string {
  return value
    .split(protectedTechnicalPattern)
    .map((part) =>
      protectedTechnicalTokenSet.has(part)
        ? `<span translate="no">${escapeHtml(part)}</span>`
        : escapeHtml(part)
    )
    .join("")
}

function renderCode(value: string): string {
  return `<code translate="no">${escapeHtml(value)}</code>`
}

function renderStatus(status: string): string {
  return `<span class="status-tag" data-status="${escapeHtml(status)}">${escapeHtml(status)}</span>`
}

function renderCase(
  kind: "normal" | "failure" | "recovery",
  value: NarrativeCase
): string {
  const label =
    kind === "normal"
      ? "正常路径"
      : kind === "failure"
        ? "失败模式"
        : "恢复路径"
  const rows = value.eventRows
    ? `<div class="mini-ledger"><table><caption class="visually-hidden">该路径对应的 durable 事件账本</caption><colgroup><col class="mini-ledger__seq-column"><col class="mini-ledger__type-column"><col></colgroup><thead><tr><th scope="col">序号</th><th scope="col">事件</th><th scope="col">事实</th></tr></thead><tbody>${value.eventRows
        .map(
          (row) => `<tr data-tone="${row.tone}">
            <td class="mini-ledger__seq">${renderCode(row.seq)}</td>
            <td>${renderCode(row.type)}</td>
            <td>${renderProse(row.fact)}</td>
          </tr>`
        )
        .join("")}</tbody></table></div>`
    : ""

  return `<article class="case" data-case="${kind}">
    <div class="case__label">${label}</div>
    <h4>${renderProse(value.title)}</h4>
    <p>${renderProse(value.summary)}</p>
    <ol>${value.steps.map((step) => `<li>${renderProse(step)}</li>`).join("")}</ol>
    ${rows}
  </article>`
}

function renderRisk(risk: RiskNote): string {
  return `<article class="risk-note" data-risk-level="${risk.level}">
    <div class="risk-note__header"><span>${risk.level === "已登记" ? "潜在风险 · 已登记" : "潜在风险 · 审阅提示"}</span><strong>${renderProse(risk.title)}</strong></div>
    <dl>
      <div><dt>触发条件</dt><dd>${renderProse(risk.trigger)}</dd></div>
      <div><dt>影响</dt><dd>${renderProse(risk.impact)}</dd></div>
      <div><dt>责任层</dt><dd>${renderProse(risk.owner)}</dd></div>
      <div><dt>验证方式</dt><dd>${renderProse(risk.verification)}</dd></div>
    </dl>
  </article>`
}

function renderBoundaries(entry: ConceptEntry): string {
  return `<div class="concept-boundaries" aria-label="${escapeHtml(entry.term)} 的延期与非目标边界">
    <article><span>明确延期</span><p>${renderProse(entry.boundaries.deferred)}</p></article>
    <article><span>非目标</span><p>${renderProse(entry.boundaries.nonGoal)}</p></article>
  </div>`
}

function renderEvidence(ref: EvidenceRef): string {
  return `<li>
    <span class="evidence__kind">${ref.kind}</span>
    ${renderCode(ref.path)}
    <strong translate="no">${escapeHtml(ref.symbol)}</strong>
    <span>${renderProse(ref.note)}</span>
  </li>`
}

function renderConcept(entry: ConceptEntry): string {
  return `<article class="concept" id="concept-${entry.id}">
    <header class="concept__header">
      <div>
        <div class="status-row">${entry.statuses.map(renderStatus).join("")}</div>
        <h3>${renderProse(entry.term)}${entry.english ? `<span translate="no">${escapeHtml(entry.english)}</span>` : ""}</h3>
      </div>
      <a class="concept__anchor" href="#context/${entry.id}" aria-label="链接到 ${escapeHtml(entry.term)}">#</a>
    </header>
    <p class="concept__definition">${renderProse(entry.definition)}</p>
    <div class="invariant"><span>强制不变量</span><p>${renderProse(entry.invariant)}</p></div>
    <div class="case-grid">
      ${renderCase("normal", entry.normal)}
      ${renderCase("failure", entry.failure)}
      ${renderCase("recovery", entry.recovery)}
    </div>
    <div class="risk-list">${entry.risks.map(renderRisk).join("")}</div>
    ${renderBoundaries(entry)}
    <details class="evidence">
      <summary>实现证据与代码定位 <span>${entry.evidence.length}</span></summary>
      <ul>${entry.evidence.map(renderEvidence).join("")}</ul>
      <div class="avoid"><span>避免使用</span>${entry.avoid.map(renderCode).join("")}</div>
    </details>
  </article>`
}

function renderChapterVisual(chapter: ManualChapter): string {
  switch (chapter.visual) {
    case "first-principles":
      return `<div class="route-visual route-visual--principles" aria-label="事实驱动的确定性恢复路线">
        <div class="route-node"><span>01</span><strong>外部结果</strong><small translate="no">LLM / Hook / Tool</small></div>
        <div class="route-arrow"><span>先固化</span></div>
        <div class="route-node route-node--active"><span>02</span><strong translate="no">Durable facts</strong><small translate="no">event stream + journal</small></div>
        <div class="route-arrow"><span>确定性重建</span></div>
        <div class="route-node"><span>03</span><strong>下一 <span translate="no">Operation</span></strong><small>唯一合法推进</small></div>
      </div>`
    case "identity-map":
      return `<div class="identity-map" aria-label="Agent definition 与多个 AgentRuntime 的身份关系">
        <div class="identity-map__definition"><small>不可变定义</small><strong translate="no">AgentId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c69</code><span translate="no">name = research-agent</span><span translate="no">version = alpha-2</span></div>
        <div class="identity-map__branch"><span>PIN</span><span>PIN</span></div>
        <div class="identity-map__runtimes">
          <div><small>长期运行聚合</small><strong translate="no">AgentRuntimeId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c6a</code><span translate="no">event_seq 1…70</span></div>
          <div><small>隔离的另一聚合</small><strong translate="no">AgentRuntimeId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c6b</code><span translate="no">event_seq 1…18</span></div>
        </div>
      </div>`
    case "iteration-ledger":
      return `<div class="event-ledger"><table><caption class="visually-hidden">一个 <span translate="no">Tool iteration</span> 的事件账本</caption><colgroup><col class="event-ledger__seq-column"><col class="event-ledger__fact-column"><col></colgroup><thead><tr><th scope="col" translate="no">event_seq</th><th scope="col" translate="no">durable fact</th><th scope="col">对 <span translate="no">committed context</span> 的影响</th></tr></thead><tbody>
        <tr><td>${renderCode("34")}</td><th scope="row" translate="no">MessageAppended · assistant tool_call</th><td>追加模型选择的 <span translate="no">Tool Call</span></td></tr>
        <tr data-tone="internal"><td>${renderCode("35")}</td><th scope="row" translate="no">ToolExecutionStarted · call_9</th><td>只证明允许外部动作开始</td></tr>
        <tr><td>${renderCode("36")}</td><th scope="row" translate="no">MessageAppended · role=tool</th><td>结果 durable 后进入基线</td></tr>
        <tr data-tone="internal"><td>${renderCode("37")}</td><th scope="row" translate="no">HookInvocationCompleted</th><td translate="no">prepare_next_turn = Continue</td></tr>
        <tr><td>${renderCode("38")}</td><th scope="row" translate="no">IterationCompleted · iteration=2</th><td>越过唯一安全边界</td></tr>
      </tbody></table></div>`
    case "hook-journal":
      return `<div class="journal-flow" aria-label="Hook journal 先记录再执行流程">
        <div><small translate="no">event 41</small><strong translate="no">Pending</strong><span translate="no">address + input digest</span></div>
        <i aria-hidden="true"></i>
        <div class="journal-flow__external"><small>非确定性边界</small><strong translate="no">handler.call()</strong><span translate="no">decision = Compact(summary)</span></div>
        <i aria-hidden="true"></i>
        <div class="journal-flow__complete"><small translate="no">event 42</small><strong translate="no">Completed</strong><span>完整 <span translate="no">decision</span> 已固化</span></div>
        <i aria-hidden="true"></i>
        <div><small translate="no">event 43</small><strong translate="no">Effect</strong><span translate="no">TranscriptCompacted</span></div>
      </div>`
    case "crash-recovery":
      return `<div class="state-machine" aria-label="运行时崩溃与恢复状态机">
        <div class="state-machine__state" data-state="running"><span>运行中</span><code translate="no">event_seq 51</code></div>
        <div class="state-machine__edge state-machine__edge--failure"><span translate="no">Started 已提交 / result 缺失</span></div>
        <div class="state-machine__state" data-state="failed"><span>进程消失</span><code translate="no">running + unhosted</code></div>
        <div class="state-machine__edge state-machine__edge--recovery"><span>显式 <span translate="no">resume + strict replay</span></span></div>
        <div class="state-machine__state" data-state="recovering"><span>恢复中</span><code translate="no">retry call_9</code></div>
        <div class="state-machine__edge"><span translate="no">result durable</span></div>
        <div class="state-machine__state" data-state="done"><span>继续运行</span><code translate="no">event_seq 52</code></div>
      </div>`
    case "compaction-modes":
      return `<div class="compaction-modes" aria-label="Compaction producer 与 consumer 证据边界">
        <section><small>当前可证明</small><h3>机制 / <span translate="no">Consumer</span></h3><p translate="no">typed Compact → marker → companion → full/window replay</p><div class="mode-track" translate="no"><span>journal</span><span>kernel</span><span>PG</span><span>resume</span></div></section>
        <div class="compaction-modes__boundary"><span>证据不能互相替代</span></div>
        <section data-deferred="true"><small>明确延期 · <span translate="no">H5b/H5c</span></small><h3>生产策略 / <span translate="no">Producer</span></h3><p>真实阈值、<span translate="no">summary provider</span>、<span translate="no">chain</span> 演进、成本与质量</p><div class="mode-track" translate="no"><span>usage</span><span>policy</span><span>summary</span><span>evidence</span></div></section>
      </div>`
  }
}

function renderChapter(chapter: ManualChapter): string {
  return `<section class="manual-chapter" id="context-${chapter.id}" data-nav-section="${chapter.id}">
    <header class="chapter-heading">
      <div><span>${renderProse(chapter.navLabel)}</span><h2>${renderProse(chapter.title)}</h2></div>
      <p>${renderProse(chapter.thesis)}</p>
    </header>
    ${renderChapterVisual(chapter)}
    <div class="concept-list">${chapter.concepts.map(renderConcept).join("")}</div>
  </section>`
}

const todoViewGroups: ReadonlyArray<{
  id: string
  label: string
  statuses: readonly TodoStatus[]
}> = [
  {
    id: "current",
    label: "当前工作",
    statuses: ["待开始", "进行中", "待讨论"],
  },
  { id: "blocked", label: "受阻", statuses: ["受阻"] },
  { id: "deferred", label: "明确延期", statuses: ["明确延期"] },
  { id: "completed", label: "已完成", statuses: ["已完成", "已取代"] },
]

function renderTodoInitiative(item: TodoInitiative): string {
  return `<article class="todo-initiative" id="todo-${item.id.toLowerCase()}" data-todo-status="${item.status}">
    <header>
      <div>${renderCode(item.id)}${renderStatus(item.status)}</div>
      <span>${renderProse(item.area)}</span>
    </header>
    <h3>${renderProse(item.title)}</h3>
    <p>${renderProse(item.goal)}</p>
    ${item.dependencies.length ? `<div class="dependencies"><span>依赖</span>${item.dependencies.map((id) => `<a href="#todo/${id.toLowerCase()}">${escapeHtml(id)}</a>`).join("")}</div>` : ""}
    <ul class="todo-items">${item.items
      .map(
        (entry) => `<li data-item-status="${entry.status}">
          ${renderCode(entry.id)}<span class="todo-item__mark" aria-hidden="true"></span><div><strong>${renderProse(entry.title)}</strong>${entry.note ? `<small>${renderProse(entry.note)}</small>` : ""}</div>${renderStatus(entry.status)}
        </li>`
      )
      .join("")}</ul>
    <details><summary>验收条件</summary><ul>${item.acceptance.map((value) => `<li>${renderProse(value)}</li>`).join("")}</ul></details>
  </article>`
}

function renderTodoGroups(): string {
  return todoViewGroups
    .map((group) => {
      const items = todoLedger.initiatives.filter((initiative) =>
        group.statuses.includes(initiative.status)
      )
      return `<section class="todo-group" id="todo-${group.id}" data-nav-section="${group.id}">
        <header class="chapter-heading"><div><span>工程待办</span><h2>${group.label}</h2></div><p>${items.length} 条工作线 · 默认只展示当前需要关注的状态</p></header>
        <div class="todo-list">${items.map(renderTodoInitiative).join("")}</div>
      </section>`
    })
    .join("")
}

function validateContent(): void {
  const errors: string[] = []
  const conceptIds = new Set<string>()
  const terms = new Set<string>()
  const knownKnowledgeStatuses = new Set<string>(knowledgeStatuses)

  for (const chapter of contextManual.chapters) {
    for (const entry of chapter.concepts) {
      if (conceptIds.has(entry.id))
        errors.push(`duplicate concept id: ${entry.id}`)
      if (terms.has(entry.term))
        errors.push(`duplicate concept term: ${entry.term}`)
      conceptIds.add(entry.id)
      terms.add(entry.term)
      if (entry.statuses.length === 0)
        errors.push(`concept has no status: ${entry.id}`)
      for (const status of entry.statuses) {
        if (!knownKnowledgeStatuses.has(status))
          errors.push(`unknown knowledge status ${status}: ${entry.id}`)
      }
      if (entry.evidence.length === 0)
        errors.push(`concept has no evidence: ${entry.id}`)
      if (entry.risks.length === 0)
        errors.push(`concept has no risk record: ${entry.id}`)
      if (!entry.boundaries.deferred.trim())
        errors.push(`concept has no deferred boundary: ${entry.id}`)
      if (!entry.boundaries.nonGoal.trim())
        errors.push(`concept has no non-goal boundary: ${entry.id}`)
      if (!entry.invariant.trim())
        errors.push(`concept has no invariant: ${entry.id}`)
    }
  }

  const initiativeIds = new Set(
    todoLedger.initiatives.map((initiative) => initiative.id)
  )
  if (initiativeIds.size !== todoLedger.initiatives.length)
    errors.push("duplicate todo initiative id")
  const itemIds = new Set<string>()
  const knownTodoStatuses = new Set<string>(todoStatuses)
  for (const initiative of todoLedger.initiatives) {
    if (!knownTodoStatuses.has(initiative.status))
      errors.push(`unknown todo status: ${initiative.id}`)
    for (const dependency of initiative.dependencies) {
      if (!initiativeIds.has(dependency))
        errors.push(`missing dependency ${dependency}: ${initiative.id}`)
    }
    for (const item of initiative.items) {
      if (itemIds.has(item.id))
        errors.push(`duplicate todo item id: ${item.id}`)
      itemIds.add(item.id)
      if (!knownTodoStatuses.has(item.status))
        errors.push(`unknown todo item status: ${item.id}`)
    }
  }

  const visiting = new Set<string>()
  const visited = new Set<string>()
  const byId = new Map(
    todoLedger.initiatives.map((initiative) => [initiative.id, initiative])
  )
  const terminalTodoStatuses = new Set(["已完成", "已取代"])
  for (const initiative of todoLedger.initiatives) {
    if (initiative.status !== "已完成") continue
    for (const dependency of initiative.dependencies) {
      const dependencyStatus = byId.get(dependency)?.status
      if (!dependencyStatus || !terminalTodoStatuses.has(dependencyStatus)) {
        errors.push(
          `completed initiative ${initiative.id} depends on non-terminal ${dependency}:${dependencyStatus ?? "missing"}`
        )
      }
    }
    for (const item of initiative.items) {
      if (!terminalTodoStatuses.has(item.status)) {
        errors.push(
          `completed initiative ${initiative.id} contains non-terminal item ${item.id}:${item.status}`
        )
      }
    }
  }
  function visit(id: string): void {
    if (visited.has(id)) return
    if (visiting.has(id)) {
      errors.push(`todo dependency cycle at ${id}`)
      return
    }
    visiting.add(id)
    for (const dependency of byId.get(id)?.dependencies ?? []) visit(dependency)
    visiting.delete(id)
    visited.add(id)
  }
  for (const id of initiativeIds) visit(id)

  if (errors.length > 0)
    throw new Error(
      `context-site validation failed:\n${errors.map((error) => `- ${error}`).join("\n")}`
    )
}

async function validateEvidence(): Promise<void> {
  const failures: string[] = []
  const sourceByPath = new Map<string, string>()

  function occurrenceIndices(source: string, needle: string): number[] {
    const indices: number[] = []
    let start = 0
    while (start < source.length) {
      const index = source.indexOf(needle, start)
      if (index < 0) break
      indices.push(index)
      start = index + needle.length
    }
    return indices
  }

  for (const chapter of contextManual.chapters) {
    for (const entry of chapter.concepts) {
      for (const reference of entry.evidence) {
        let source = sourceByPath.get(reference.path)
        try {
          if (source === undefined) {
            source = await readFile(
              resolve(repositoryRoot, reference.path),
              "utf8"
            )
            sourceByPath.set(reference.path, source)
          }
          const anchorIndices = occurrenceIndices(source, reference.symbol)
          const contextIndices = occurrenceIndices(source, reference.context)
          const evidenceWindow = 2_500
          if (anchorIndices.length === 0) {
            failures.push(
              `${entry.id}: symbol ${reference.symbol} is missing from ${reference.path}`
            )
          } else if (
            !anchorIndices.some((anchorIndex) =>
              contextIndices.some(
                (contextIndex) =>
                  Math.abs(contextIndex - anchorIndex) <= evidenceWindow
              )
            )
          ) {
            failures.push(
              `${entry.id}: context ${reference.context} is not anchored near ${reference.symbol} in ${reference.path}`
            )
          }
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error)
          failures.push(
            `${entry.id}: cannot read ${reference.path}: ${message}`
          )
        }
      }
    }
  }

  if (failures.length > 0) {
    throw new Error(
      `context-site evidence validation failed:\n${failures.map((failure) => `- ${failure}`).join("\n")}`
    )
  }
}

async function buildHtml(): Promise<string> {
  validateContent()
  await validateEvidence()
  const [styles, runtime, globalStyles, gsapRuntime] = await Promise.all([
    readFile(resolve(scriptDirectory, "site.css"), "utf8"),
    readFile(resolve(scriptDirectory, "runtime.js"), "utf8"),
    readFile(resolve(webRoot, "app/globals.css"), "utf8"),
    readFile(resolve(webRoot, "node_modules/gsap/dist/gsap.min.js"), "utf8"),
  ])

  const darkTokenStart = globalStyles.indexOf(".dark {")
  if (darkTokenStart < 0) throw new Error("stratum-web dark tokens are missing")
  let depth = 0
  let darkTokenEnd = -1
  for (
    let index = globalStyles.indexOf("{", darkTokenStart);
    index < globalStyles.length;
    index += 1
  ) {
    if (globalStyles[index] === "{") depth += 1
    if (globalStyles[index] === "}") {
      depth -= 1
      if (depth === 0) {
        darkTokenEnd = index + 1
        break
      }
    }
  }
  if (darkTokenEnd < 0)
    throw new Error("stratum-web dark token block is malformed")
  const sharedDarkTokens = globalStyles
    .slice(darkTokenStart, darkTokenEnd)
    .replace(/^\.dark/, ":root")
  const contextNav = contextManual.chapters
    .map(
      (chapter) =>
        `<a href="#context/${chapter.id}" data-nav-target="${chapter.id}" aria-label="${escapeHtml(chapter.navLabel)}"><span class="dock-glyph" aria-hidden="true">${chapter.navLabel.slice(0, 1)}</span><span class="dock-tooltip">${renderProse(chapter.navLabel)}</span></a>`
    )
    .join("")
  const todoNav = todoViewGroups
    .map(
      (group) =>
        `<a href="#todo/${group.id}" data-nav-target="${group.id}" aria-label="${escapeHtml(group.label)}"><span class="dock-glyph" aria-hidden="true">${group.label.slice(0, 1)}</span><span class="dock-tooltip">${escapeHtml(group.label)}</span></a>`
    )
    .join("")

  const currentCount = todoLedger.initiatives.filter((item) =>
    ["待开始", "进行中", "待讨论"].includes(item.status)
  ).length
  const blockedCount = todoLedger.initiatives.filter(
    (item) => item.status === "受阻"
  ).length
  const deferredCount = todoLedger.initiatives.filter(
    (item) => item.status === "明确延期"
  ).length

  return `<!doctype html>
<html lang="zh-CN" class="dark">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark">
  <meta name="theme-color" content="#0d0f0e">
  <title>${escapeHtml(contextManual.title)}</title>
  <meta name="description" content="${escapeHtml(contextManual.description)}">
  <style>${sharedDarkTokens}\n${styles}</style>
</head>
<body>
  <!--
    THESIS: 以一条可追踪的运行路线解释 Stratum，而非把术语拆成普通文档卡片。
    OWN-WORLD: 暗色搪瓷线路图 + 事件账本；绿=事实/不变量，蓝=恢复，琥珀=风险，红=失败。
    STORY: 读者从 identity 与 event stream 出发，经历 Tool/Hook/crash/compaction，再落到风险与待办。
    FIRST VIEWPORT: 左侧波形 dock、顶部双页签、中央运行路线首屏、右侧版本与阅读协议。
    FORM: 候选 A 纵向运行路线为骨架，嵌入候选 B 事件账本与候选 C 状态机；surface seed ca08474b。
  -->
  <a class="skip-link" href="#main-content">跳到正文</a>
  <div class="ambient-route" aria-hidden="true"><span></span><span></span><span></span></div>
  <header class="site-header">
    <a class="brand" href="#context/first-principles"><span class="brand-mark" translate="no">S</span><span><strong translate="no">Stratum</strong><small>运行时现场手册</small></span></a>
    <div class="surface-tabs" role="tablist" aria-label="知识视图">
      <button id="tab-context" type="button" role="tab" aria-controls="surface-context" aria-selected="true" data-surface-tab="context">领域手册</button>
      <button id="tab-todo" type="button" role="tab" aria-controls="surface-todo" aria-selected="false" data-surface-tab="todo">工程待办</button>
    </div>
    <div class="build-stamp"><span>只读制品</span>${renderCode(contextManual.checkedAtCommit)}</div>
  </header>
  <nav class="side-dock" aria-label="当前视图章节">
    <div data-dock-panel="context">${contextNav}</div>
    <div data-dock-panel="todo" hidden>${todoNav}</div>
  </nav>
  <main id="main-content">
    <div class="surface" id="surface-context" role="tabpanel" aria-labelledby="tab-context" data-surface="context">
      <section class="manual-hero" id="context-overview">
        <div class="hero-copy">
          <div class="hero-kicker"><span></span>${renderProse("工程师与 Agent 的共同运行地图")}</div>
          <h1>沿着事实，<br><em>恢复下一步。</em></h1>
          <p>${renderProse(contextManual.description)}</p>
          <div class="hero-actions"><a href="#context/first-principles">开始阅读</a><button type="button" aria-expanded="false" data-open-depth>展开全部证据</button></div>
        </div>
        <div class="hero-route" aria-label="一次 AgentRuntime 从定义到恢复的路线">
          <div class="hero-route__line"></div>
          <div class="hero-station" data-tone="fact"><span></span><div><small>定义固定</small><strong translate="no">AgentId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c69</code></div></div>
          <div class="hero-station" data-tone="fact"><span></span><div><small>运行聚合</small><strong translate="no">AgentRuntimeId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c6a</code></div></div>
          <div class="hero-station" data-tone="internal"><span></span><div><small>有序事实权威</small><strong translate="no">Durable ledger</strong><code translate="no">event_seq 1…70</code></div></div>
          <div class="hero-station" data-tone="failure"><span></span><div><small>故障边界</small><strong translate="no">process lost</strong><code translate="no">running + unhosted</code></div></div>
          <div class="hero-station" data-tone="recovery"><span></span><div><small>显式恢复</small><strong translate="no">strict replay</strong><code translate="no">resume exact Turn</code></div></div>
        </div>
        <aside class="reading-protocol">
          <span>阅读协议</span>
          <dl><div><dt>内容状态</dt><dd>中文标签区分事实、风险与延期</dd></div><div><dt>示例数据</dt><dd>${renderProse("全部为合成 identity，不对应真实运行")}</dd></div><div><dt>代码定位</dt><dd>${renderProse("路径 + symbol，不固定易漂移行号")}</dd></div><div><dt>核对基准</dt><dd>${renderCode(contextManual.checkedAtCommit)}</dd></div></dl>
        </aside>
      </section>
      <div class="manual-route"><div class="manual-route__rail" aria-hidden="true"></div>${contextManual.chapters.map(renderChapter).join("")}</div>
      <footer class="manual-footer"><div><strong>知识只有一个人工维护源。</strong><p>经确认的领域结论更新 context-site；本 HTML 由构建生成，禁止手改。</p></div><a href="#context/first-principles">回到起点</a></footer>
    </div>
    <div class="surface" id="surface-todo" role="tabpanel" aria-labelledby="tab-todo" data-surface="todo" hidden>
      <section class="todo-hero" id="todo-overview">
        <div><div class="hero-kicker"><span></span>唯一工程待办账本</div><h1>${escapeHtml(todoLedger.title)}</h1><p>默认聚焦当前工作；受阻、延期、已完成与被取代历史均保留稳定 <span translate="no">ID</span> 与验收条件。</p></div>
        <div class="todo-summary"><div><strong>${currentCount}</strong><span>当前工作线</span></div><div><strong>${blockedCount}</strong><span>受阻</span></div><div><strong>${deferredCount}</strong><span>明确延期</span></div></div>
      </section>
      <section class="execution-principles" aria-labelledby="execution-principles-title"><div><span>执行原则</span><h2 id="execution-principles-title">如何使用这份待办</h2></div><ol>${todoLedger.principles.map((value) => `<li>${renderProse(value)}</li>`).join("")}</ol></section>
      <section class="coordination-map" aria-labelledby="coordination-title">
        <header><span>协作顺序</span><h2 id="coordination-title">哪些能并行，哪些必须串行</h2><p>${renderProse("依赖关系不是进度装饰；它定义了人和 Agent 可以安全分工的边界。")}</p></header>
        <div><article><h3>可并行推进</h3><ol>${todoLedger.coordination.parallelTracks.map((value) => `<li>${renderProse(value)}</li>`).join("")}</ol></article><article><h3>必须串行</h3><ol>${todoLedger.coordination.serialRules.map((value) => `<li>${renderProse(value)}</li>`).join("")}</ol></article></div>
      </section>
      ${renderTodoGroups()}
      <section class="deferred-boundaries"><div><span>边界账本</span><h2>不会被“顺手实现”的事项</h2></div><ul>${todoLedger.deferredBoundaries.map((value) => `<li>${renderProse(value)}</li>`).join("")}</ul></section>
    </div>
  </main>
  <script>${gsapRuntime}</script>
  <script>${runtime}</script>
</body>
</html>`
}

const html = await buildHtml()

function validateGeneratedHtml(value: string): void {
  const errors: string[] = []
  const ids = new Set<string>()
  for (const match of value.matchAll(/\sid="([^"]+)"/g)) {
    const id = match[1]
    if (ids.has(id)) errors.push(`duplicate generated id: ${id}`)
    ids.add(id)
  }

  for (const match of value.matchAll(/href="#([^"]+)"/g)) {
    const route = match[1]
    const [surface, target] = route.split("/")
    const candidates =
      surface === "context"
        ? [`context-${target}`, `concept-${target}`]
        : surface === "todo"
          ? [`todo-${target}`]
          : [route]
    if (!candidates.some((candidate) => ids.has(candidate))) {
      errors.push(`broken generated hash route: #${route}`)
    }
  }

  if (/<(?:script|img)[^>]+\ssrc=/i.test(value) || /<link\b/i.test(value)) {
    errors.push("generated artifact contains an external asset element")
  }
  if (/\s(?:href|src)="https?:\/\//i.test(value)) {
    errors.push("generated artifact contains a network URL")
  }
  if (/<iframe\b/i.test(value)) {
    errors.push("generated artifact contains an embedded browsing context")
  }
  if (
    /\b(?:fetch|EventSource|WebSocket|XMLHttpRequest|sendBeacon)\s*\(/.test(
      value
    )
  ) {
    errors.push("generated artifact contains a runtime network primitive")
  }
  if (/@import\b/i.test(value) || /url\(\s*['\"]?https?:\/\//i.test(value)) {
    errors.push("generated artifact contains a CSS network dependency")
  }
  if (!/<meta name="theme-color" content="#[0-9a-f]{6}">/i.test(value)) {
    errors.push("generated artifact is missing a concrete theme color")
  }
  if (/<code(?![^>]*\btranslate="no")[^>]*>/i.test(value)) {
    errors.push("generated artifact contains a translatable code token")
  }

  const bodyStart = value.indexOf("<body>")
  const scriptStart = value.indexOf("<script>", bodyStart)
  const bodyMarkup = value.slice(bodyStart, scriptStart)
  const translateStack: boolean[] = []
  const voidElements = new Set(["br", "col", "hr", "img", "input", "wbr"])
  const uncoveredTokens = new Map<string, string>()
  for (const chunk of bodyMarkup.match(/<!--[\s\S]*?-->|<[^>]+>|[^<]+/g) ??
    []) {
    if (chunk.startsWith("<!--")) continue
    if (chunk.startsWith("</")) {
      translateStack.pop()
      continue
    }
    if (chunk.startsWith("<")) {
      const name = /^<\s*([a-z0-9-]+)/i.exec(chunk)?.[1]?.toLowerCase()
      if (!name || chunk.startsWith("<!")) continue
      const inherited = translateStack.at(-1) ?? false
      const protectedHere = inherited || /\btranslate="no"/i.test(chunk)
      if (!voidElements.has(name) && !/\/>$/.test(chunk)) {
        translateStack.push(protectedHere)
      }
      continue
    }
    if (translateStack.at(-1)) continue
    for (const token of protectedTechnicalTokens) {
      if (chunk.includes(token) && !uncoveredTokens.has(token)) {
        uncoveredTokens.set(token, chunk.trim().slice(0, 120))
      }
    }
  }
  if (uncoveredTokens.size > 0) {
    errors.push(
      `generated artifact contains translatable technical tokens: ${[
        ...uncoveredTokens,
      ]
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([token, sample]) => `${token} in ${JSON.stringify(sample)}`)
        .join("; ")}`
    )
  }

  if (errors.length > 0) {
    throw new Error(
      `context-site artifact validation failed:\n${errors.map((error) => `- ${error}`).join("\n")}`
    )
  }
}

const normalizedHtml = html.replace(/[ \t]+$/gm, "")
validateGeneratedHtml(normalizedHtml)
await writeFile(outputPath, normalizedHtml, "utf8")
console.log(`generated ${outputPath}`)
