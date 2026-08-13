import { execFile } from "node:child_process"
import { constants } from "node:fs"
import { access } from "node:fs/promises"
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { promisify } from "node:util"

import { contextManual } from "./content/context.ts"
import {
  knowledgeStatuses,
  todoStatuses,
  type ConceptEntry,
  type EvidenceRef,
  type KernelPrimer,
  type ManualChapter,
  type NarrativeCase,
  type ReActLoopGuide,
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
const execFileAsync = promisify(execFile)

function mermaidLabel(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll("\n", " ")
}

async function firstExecutable(
  candidates: readonly (string | undefined)[]
): Promise<string | undefined> {
  for (const candidate of candidates) {
    if (!candidate) continue
    try {
      await access(candidate, constants.X_OK)
      return candidate
    } catch {
      // Try the next platform-specific system Chrome location.
    }
  }
  return undefined
}

async function resolveContextSiteChrome(): Promise<string> {
  const environmentCandidate =
    process.env.CONTEXT_SITE_CHROME_PATH ??
    process.env.PUPPETEER_EXECUTABLE_PATH
  const platformCandidates =
    process.platform === "darwin"
      ? [
          "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
          "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
      : process.platform === "win32"
        ? [
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
          ]
        : [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
          ]
  const executable = await firstExecutable([
    environmentCandidate,
    ...platformCandidates,
  ])
  if (executable) return executable
  throw new Error(
    "context-site Mermaid rendering needs a system Chrome or Chromium binary; set CONTEXT_SITE_CHROME_PATH to its executable path"
  )
}

interface ReActMermaidDiagram {
  readonly id: "execution" | "durable" | "recovery"
  readonly title: string
  readonly caption: string
  readonly source: string
}

function buildReActMermaidDiagrams(
  guide: ReActLoopGuide
): readonly ReActMermaidDiagram[] {
  const steps = new Map(guide.steps.map((step) => [step.id, step]))
  const hooks = new Map(guide.hooks.map((hook) => [hook.id, hook]))
  const persistence = new Map(
    guide.persistence.map((point) => [point.id, point])
  )
  const step = (id: string) => {
    const value = steps.get(id)
    if (!value) throw new Error(`missing ReAct step for Mermaid: ${id}`)
    return `${mermaidLabel(value.label)}<br/>${mermaidLabel(value.term)}`
  }
  const hook = (id: string) => {
    const value = hooks.get(id)
    if (!value) throw new Error(`missing ReAct hook for Mermaid: ${id}`)
    return mermaidLabel(value.term)
  }
  const event = (id: string) => {
    const value = persistence.get(id)
    if (!value)
      throw new Error(`missing ReAct persistence point for Mermaid: ${id}`)
    return mermaidLabel(value.events)
  }

  const styling = `
  classDef input fill:#171c1d,stroke:#5a6a6d,color:#e7eceb,stroke-width:1px
  classDef runtime fill:#16191d,stroke:#526887,color:#e7eceb,stroke-width:1px
  classDef hook fill:#252118,stroke:#c69a57,color:#f4e7c8,stroke-width:1px
  classDef decision fill:#20231b,stroke:#8dac64,color:#e4efd5,stroke-width:1px
  classDef fact fill:#14231c,stroke:#6ca777,color:#e1f0e1,stroke-width:1px
  classDef failure fill:#29191b,stroke:#b65f67,color:#f7d9dd,stroke-width:1px
  classDef recovery fill:#17212a,stroke:#6696b9,color:#d6e9f6,stroke-width:1px
  classDef tail fill:#211927,stroke:#8d6aa9,color:#eadcf7,stroke-width:1px
  linkStyle default stroke:#788788,stroke-width:1.25px,fill:none
`

  return [
    {
      id: "execution",
      title: "① 运行主线：ReAct 怎么推进",
      caption:
        "从新 Turn 到终态的正常路径。琥珀色是五个 Hook；绿色是迭代边界或继续判断；红色是失败、取消等终止分支。",
      source: `flowchart TB
  subgraph composition["组合层：每次 fresh / resume 都要重新提供"]
    definition["Agent definition + TurnRuntimeSnapshot"]
    components["LLM provider · Tool registry · Hook chain · LoopLimits"]
  end
  subgraph kernel["ReAct kernel：AgentLoop 的内存状态机"]
    entry["${step("entry")}"]
    context["${hook("transform-context")}"]
    request["request view：只供本次模型请求"]
    model["${step("model")}"]
    assistant["${step("assistant")}"]
    hasTool{"Tool call？"}
    noToolBoundary["IterationCompleted"]
    transformTool["${hook("transform-tool")}"]
    decideTool["${hook("decide-tool")}"]
    approval["人工审批等待"]
    toolStarted["ToolExecutionStarted"]
    externalTool["外部 Tool 调用"]
    afterTool["${hook("after-tool")}"]
    toolResult["role=tool MessageAppended"]
    prepareNext["${hook("prepare-next")}"]
    toolBoundary["${step("boundary")}"]
    continueTurn{"继续下一轮？"}
    finished["LoopFinished"]
    stopped["LoopFailed / LoopCancelled"]
  end
  definition --> entry
  components --> context
  entry --> context --> request --> model --> assistant --> hasTool
  hasTool -->|没有 Tool| noToolBoundary --> finished
  hasTool -->|有 Tool| transformTool --> decideTool --> toolStarted --> externalTool --> afterTool --> toolResult --> prepareNext --> toolBoundary --> continueTurn
  decideTool -.需要人工决定.-> approval --> decideTool
  continueTurn -->|继续| context
  continueTurn -->|Stop| finished
  context -.Hook / provider 失败或取消.-> stopped
  model -.模型失败或取消.-> stopped
  externalTool -.Tool 失败或取消.-> stopped
  class definition,components input
  class entry,request,model,assistant,externalTool runtime
  class context,transformTool,decideTool,afterTool,prepareNext hook
  class hasTool,continueTurn,noToolBoundary,toolBoundary decision
  class finished,stopped failure
${styling}`,
    },
    {
      id: "durable",
      title: "② 持久化边界：什么真正进入流水账",
      caption:
        "绿色节点是事务提交后才成立的 durable facts。紫色节点只帮助在线界面更快显示，它们可丢，不能拿来恢复。",
      source: `flowchart TB
  turnStart["新 Turn"] --> startFact["${event("turn-start")}"] --> ledger["event stream：同一 AgentRuntime 按 event_seq 排列"]
  hookCall["每次 Hook 调用"] --> hookJournal["${event("hook-journal")}"] --> ledger
  modelDone["LLM 产生完整答复"] --> assistantFact["${event("assistant-result")}"] --> ledger
  approvalOrStart["人工审批或外部 Tool 前"] --> approvalFact["${event("approval-tool-start")}"] --> ledger
  toolCycleEnd["Tool 返回、准备进入下一轮"] --> boundaryFact["${event("tool-result-boundary")}"] --> ledger
  terminal["运行终止"] --> terminalFact["${event("terminal")}"] --> ledger
  subgraph volatile["不属于 durable facts"]
    delta["LLM 开始、token delta、浏览器草稿"]
    tail["NATS / SSE 短尾"]
  end
  ledger -.commit 后发布.-> tail
  class turnStart,hookCall,modelDone,approvalOrStart,toolCycleEnd,terminal runtime
  class startFact,hookJournal,assistantFact,approvalFact,boundaryFact,terminalFact,ledger fact
  class delta,tail tail
${styling}`,
    },
    {
      id: "recovery",
      title: "③ 恢复边界：进程丢失后怎么继续",
      caption:
        "恢复不是重放浏览器或 NATS。API 先同时验证 runtime 状态、固定 definition 与 snapshot，再严格读取同一条 durable ledger，最后重新装配 kernel。",
      source: `flowchart LR
  state["agent_states：当前 runtime / session / Turn fence"] --> fence["三方 pin 校验"]
  definition["agents：不可变 Agent definition"] --> fence
  snapshot["LoopStarted：TurnRuntimeSnapshot"] --> fence
  ledger["event stream：durable facts"] --> strictRead["strict read：version / shape / identity 连续性"]
  fence --> replay["replay 已确认的事实"]
  strictRead --> replay
  replay --> reassemble["重新装配 AgentLoop\n得到下一件合法 Operation"]
  reassemble --> next["继续执行，新的事实再写回 ledger"]
  tail["NATS / SSE / 浏览器草稿"] -.不作为恢复输入.-> strictRead
  class state,definition,snapshot input
  class ledger fact
  class fence,strictRead,replay,reassemble,next recovery
  class tail tail
${styling}`,
    },
  ]
}

async function renderReActMermaid(
  guide: ReActLoopGuide
): Promise<
  Readonly<
    Record<ReActMermaidDiagram["id"], ReActMermaidDiagram & { svg: string }>
  >
> {
  const temporaryDirectory = await mkdtemp(
    resolve(tmpdir(), "stratum-react-mermaid-")
  )
  const puppeteerConfigPath = resolve(temporaryDirectory, "puppeteer.json")
  try {
    const chromePath = await resolveContextSiteChrome()
    const mermaidConfig = {
      flowchart: {
        curve: "linear",
        htmlLabels: true,
        nodeSpacing: 28,
        rankSpacing: 46,
        useMaxWidth: true,
      },
      fontFamily:
        "ui-sans-serif, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
      securityLevel: "strict",
      theme: "base",
      themeVariables: {
        background: "transparent",
        fontSize: "15px",
        lineColor: "#788788",
        primaryBorderColor: "#526887",
        primaryColor: "#16191d",
        primaryTextColor: "#e7eceb",
        secondaryBorderColor: "#6ca777",
        secondaryColor: "#14231c",
        secondaryTextColor: "#e1f0e1",
        tertiaryBorderColor: "#6696b9",
        tertiaryColor: "#17212a",
        tertiaryTextColor: "#d6e9f6",
      },
    }
    await writeFile(
      puppeteerConfigPath,
      JSON.stringify({ executablePath: chromePath, headless: true }),
      "utf8"
    )
    const diagrams = buildReActMermaidDiagrams(guide)
    const rendered = new Map<
      ReActMermaidDiagram["id"],
      ReActMermaidDiagram & { svg: string }
    >()
    for (const diagram of diagrams) {
      const inputPath = resolve(temporaryDirectory, `${diagram.id}.mmd`)
      const outputPath = resolve(temporaryDirectory, `${diagram.id}.svg`)
      const diagramConfigPath = resolve(
        temporaryDirectory,
        `${diagram.id}.json`
      )
      await Promise.all([
        writeFile(inputPath, diagram.source, "utf8"),
        writeFile(
          diagramConfigPath,
          JSON.stringify({
            ...mermaidConfig,
            deterministicIds: true,
            deterministicIDSeed: `stratum-react-${diagram.id}-v1`,
          }),
          "utf8"
        ),
      ])
      await execFileAsync(
        process.execPath,
        [
          resolve(webRoot, "node_modules/@mermaid-js/mermaid-cli/src/cli.js"),
          "--input",
          inputPath,
          "--output",
          outputPath,
          "--outputFormat",
          "svg",
          "--backgroundColor",
          "transparent",
          "--configFile",
          diagramConfigPath,
          "--puppeteerConfigFile",
          puppeteerConfigPath,
          "--svgId",
          `react-loop-${diagram.id}`,
          "--quiet",
        ],
        { cwd: webRoot, maxBuffer: 4 * 1024 * 1024 }
      )
      const svg = await readFile(outputPath, "utf8")
      if (!svg.includes("<svg") || /<script\b/i.test(svg)) {
        throw new Error(`Mermaid did not emit a safe ${diagram.id} inline SVG`)
      }
      rendered.set(diagram.id, {
        ...diagram,
        svg: svg.replace(/^<\?xml[^>]*>\s*/i, ""),
      })
    }
    const get = (id: ReActMermaidDiagram["id"]) => {
      const diagram = rendered.get(id)
      if (!diagram) throw new Error(`Mermaid diagram was not rendered: ${id}`)
      return diagram
    }
    return {
      execution: get("execution"),
      durable: get("durable"),
      recovery: get("recovery"),
    }
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error)
    throw new Error(`failed to render the ReAct Mermaid diagram: ${detail}`)
  } finally {
    await rm(temporaryDirectory, { force: true, recursive: true })
  }
}

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
  "ToolApprovalRequested",
  "ToolApprovalResolved",
  "TranscriptCompacted",
  "PrepareNextTurnDecision",
  "ExtensionSetVersionId",
  "WorkflowVersionId",
  "AgentRuntimeView",
  "AgentRuntimeId",
  "AgentRuntime",
  "TurnRuntimeSnapshot",
  "DurableAgentEvent",
  "DurableEventSink",
  "AgentLoop",
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
  "LoopFailed",
  "LoopCancelled",
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
  "durable ledger",
  "event stream",
  "COMMIT",
  "SIGTERM",
  "Postgres",
  "NATS",
  "NATS short tail",
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

function renderKernelPrimer(
  primer: KernelPrimer,
  reactMermaid: Awaited<ReturnType<typeof renderReActMermaid>>
): string {
  const stages = new Map(primer.stages.map((stage) => [stage.id, stage]))
  const stage = (id: string) => {
    const value = stages.get(id)
    if (!value) throw new Error(`missing kernel stage: ${id}`)
    return `<article class="kernel-stage" data-stage="${escapeHtml(value.id)}" data-tone="${escapeHtml(value.tone)}">
      <span>${renderProse(value.label)}</span>
      <strong>${renderProse(value.term)}</strong>
      <p>${renderProse(value.detail)}</p>
    </article>`
  }

  const reactHooks = new Map(
    primer.reactLoop.hooks.map((hook) => [hook.id, hook])
  )
  const reactHook = (id: string) => {
    const value = reactHooks.get(id)
    if (!value) throw new Error(`missing ReAct hook: ${id}`)
    return `<article class="react-loop__hook">
      <strong>${renderProse(value.term)}</strong>
      <p>${renderProse(value.purpose)}</p>
      <dl><div><dt>何时调用</dt><dd>${renderProse(value.timing)}</dd></div><div><dt>会写入</dt><dd>${renderProse(value.durable)}</dd></div></dl>
    </article>`
  }
  const renderMermaidDiagram = (
    diagram: (typeof reactMermaid)[ReActMermaidDiagram["id"]]
  ) =>
    `<section class="react-loop__diagram-panel" aria-labelledby="react-loop-${escapeHtml(diagram.id)}-title">
      <header><span translate="no">Mermaid · ${escapeHtml(diagram.id)}</span><h4 id="react-loop-${escapeHtml(diagram.id)}-title">${renderProse(diagram.title)}</h4></header>
      <figure class="react-loop__diagram" aria-labelledby="react-loop-${escapeHtml(diagram.id)}-caption">
        <figcaption id="react-loop-${escapeHtml(diagram.id)}-caption">${renderProse(diagram.caption)}<span class="react-loop__diagram-scroll-hint">窄屏可在图内横向拖动查看全图。</span></figcaption>
        <div class="react-loop__mermaid" tabindex="0" translate="no" aria-label="${escapeHtml(diagram.title)}">
          ${diagram.svg}
        </div>
      </figure>
    </section>`

  return `<section class="manual-chapter kernel-primer" id="context-${primer.id}" data-nav-section="${primer.id}">
    <header class="chapter-heading">
      <div><span>${renderProse(primer.navLabel)}</span><h2>${renderProse(primer.title)}</h2></div>
      <p>${renderProse(primer.thesis)}</p>
    </header>
    <figure class="kernel-map" aria-labelledby="kernel-map-caption">
      <figcaption id="kernel-map-caption">${renderProse("一次运行的事实循环：执行发生在组合层；确认后的结果回到 durable event stream；恢复只从已确认事实开始。")}</figcaption>
      <div class="kernel-map__lane kernel-map__lane--boot">
        ${stage("runtime")}
        <div class="kernel-map__arrow" aria-hidden="true"><span>读取边界</span></div>
        ${stage("replay")}
        <div class="kernel-map__arrow" aria-hidden="true"><span>构造</span></div>
        ${stage("kernel")}
      </div>
      <div class="kernel-map__lane kernel-map__lane--execute">
        ${stage("operation")}
        <div class="kernel-map__arrow" aria-hidden="true"><span>提交事实</span></div>
        ${stage("facts")}
      </div>
      <p class="kernel-map__cycle"><span>重启时</span><span translate="no">event stream → strict replay → AgentLoop</span></p>
      <aside class="kernel-map__tail"><strong translate="no">NATS short tail</strong><p>只把近期变化推给在线界面；它丢失时由 <span translate="no">PG view/history</span> 对账，不进入恢复输入。</p></aside>
    </figure>
    <section class="react-loop" aria-labelledby="react-loop-title">
      <header class="react-loop__heading"><span translate="no">ReAct</span><h3 id="react-loop-title">${renderProse(primer.reactLoop.title)}</h3><p>${renderProse(primer.reactLoop.thesis)}</p></header>
      <p class="react-loop__diagram-intro">三张图合起来描述同一件事：<span translate="no">kernel</span> 怎么推进、哪些结果成为 <span translate="no">durable facts</span>、以及进程丢失后为什么只能从这些事实继续。它们由 typed ReAct 内容在构建时编译为内联 <span translate="no">SVG</span>；实线是正常推进，虚线表示写入、等待或恢复边界。</p>
      <div class="react-loop__diagrams">
        ${renderMermaidDiagram(reactMermaid.execution)}
        ${renderMermaidDiagram(reactMermaid.durable)}
        ${renderMermaidDiagram(reactMermaid.recovery)}
      </div>
      <section class="react-loop__hooks" aria-labelledby="react-loop-hooks-title">
        <header><span>${renderProse("五个 Hook")}</span><h3 id="react-loop-hooks-title">它们不是随处可插的回调</h3><p>${renderProse("每个 Hook 都有固定位置、只读 snapshot 和受限的 decision。调用过程本身会进 journal。")}</p></header>
        <div>${primer.reactLoop.hooks.map((hook) => reactHook(hook.id)).join("")}</div>
      </section>
      <section class="react-loop__persistence" aria-labelledby="react-loop-persistence-title">
        <header><span>持久化点</span><h3 id="react-loop-persistence-title">哪些东西真的写进 <i translate="no">event stream</i></h3><p>这些是恢复会读取的记录。网络推送和浏览器草稿不在这里。</p></header>
        <div class="react-loop__ledger"><table><caption class="visually-hidden">ReAct 循环中的 durable event 写入点</caption><thead><tr><th scope="col">发生在什么时候</th><th scope="col">写入的事件</th><th scope="col">为什么要写</th></tr></thead><tbody>${primer.reactLoop.persistence
          .map(
            (point) =>
              `<tr id="react-persistence-${escapeHtml(point.id)}"><th scope="row">${renderProse(point.when)}</th><td>${renderProse(point.events)}</td><td>${renderProse(point.detail)}</td></tr>`
          )
          .join("")}</tbody></table></div>
      </section>
      <aside class="react-loop__note"><span>别混在一起</span><p>${renderProse(primer.reactLoop.note)}</p></aside>
    </section>
    <section class="kernel-glossary" aria-labelledby="kernel-glossary-title">
      <header><span>沿图认词</span><h3 id="kernel-glossary-title">每个名词都先回答：它在哪一层？</h3><p>${renderProse("先区分“已发生的事实”和“正在运行的进程”，再阅读后面的状态机、Hook 与恢复细节。")}</p></header>
      <dl>${primer.glossary
        .map(
          (entry) =>
            `<div><dt>${renderProse(entry.term)}</dt><dd>${renderProse(entry.definition)}</dd><dd><span>不是</span>${renderProse(entry.not)}</dd></div>`
        )
        .join("")}</dl>
    </section>
    <aside class="kernel-primer__invariant"><span>先记这一条</span><p>${renderProse(primer.invariant)}</p></aside>
    <details class="evidence">
      <summary>这张结构图的实现证据 <span>${primer.evidence.length}</span></summary>
      <ul>${primer.evidence.map(renderEvidence).join("")}</ul>
    </details>
  </section>`
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
  if (!contextManual.primer.id.trim()) errors.push("kernel primer has no id")
  if (!contextManual.primer.navLabel.trim())
    errors.push("kernel primer has no navigation label")
  if (contextManual.primer.stages.length !== 5)
    errors.push("kernel primer must have exactly five stages")
  if (contextManual.primer.reactLoop.steps.length !== 7)
    errors.push("ReAct loop must have exactly seven steps")
  if (contextManual.primer.reactLoop.hooks.length !== 5)
    errors.push("ReAct loop must document exactly five hooks")
  if (contextManual.primer.reactLoop.persistence.length === 0)
    errors.push("ReAct loop has no persistence points")
  for (const [kind, entries] of [
    ["ReAct step", contextManual.primer.reactLoop.steps],
    ["ReAct hook", contextManual.primer.reactLoop.hooks],
    ["persistence point", contextManual.primer.reactLoop.persistence],
  ] as const) {
    const identifiers = new Set<string>()
    for (const entry of entries) {
      if (!entry.id.trim()) errors.push(`${kind} has no id`)
      if (identifiers.has(entry.id))
        errors.push(`duplicate ${kind} id: ${entry.id}`)
      identifiers.add(entry.id)
    }
  }
  if (contextManual.primer.glossary.length === 0)
    errors.push("kernel primer has no glossary entries")
  if (contextManual.primer.evidence.length === 0)
    errors.push("kernel primer has no evidence")
  const conceptIds = new Set<string>()
  const terms = new Set<string>()
  const knownKnowledgeStatuses = new Set<string>(knowledgeStatuses)

  for (const chapter of contextManual.chapters) {
    if (chapter.id === contextManual.primer.id)
      errors.push(`kernel primer duplicates chapter id: ${chapter.id}`)
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

  const documentedEntries = [
    { id: contextManual.primer.id, evidence: contextManual.primer.evidence },
    ...contextManual.chapters.flatMap((chapter) => chapter.concepts),
  ]
  for (const entry of documentedEntries) {
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
        failures.push(`${entry.id}: cannot read ${reference.path}: ${message}`)
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
  const [styles, runtime, globalStyles, gsapRuntime, reactMermaid] =
    await Promise.all([
      readFile(resolve(scriptDirectory, "site.css"), "utf8"),
      readFile(resolve(scriptDirectory, "runtime.js"), "utf8"),
      readFile(resolve(webRoot, "app/globals.css"), "utf8"),
      readFile(resolve(webRoot, "node_modules/gsap/dist/gsap.min.js"), "utf8"),
      renderReActMermaid(contextManual.primer.reactLoop),
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
  const contextSections = [contextManual.primer, ...contextManual.chapters]
  const contextNav = contextSections
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
    <a class="brand" href="#context/kernel-map"><span class="brand-mark" translate="no">S</span><span><strong translate="no">Stratum</strong><small>运行时现场手册</small></span></a>
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
          <div class="hero-actions"><a href="#context/kernel-map">先看核心结构</a><button type="button" aria-expanded="false" data-open-depth>展开全部证据</button></div>
        </div>
        <div class="hero-route" aria-label="一次 AgentRuntime 从定义到恢复的路线">
          <div class="hero-route__line"></div>
          <div class="hero-station" data-tone="fact"><span></span><div><small>定义固定</small><strong translate="no">AgentId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c69</code></div></div>
          <div class="hero-station" data-tone="fact"><span></span><div><small>运行聚合</small><strong translate="no">AgentRuntimeId</strong><code translate="no">019fd245-de54-7533-87bf-cc33628c6c6a</code></div></div>
          <div class="hero-station" data-tone="internal"><span></span><div><small>有序事实权威</small><strong>事件账本</strong><code translate="no">event stream · event_seq 1…70</code></div></div>
          <div class="hero-station" data-tone="failure"><span></span><div><small>故障边界</small><strong translate="no">process lost</strong><code translate="no">running + unhosted</code></div></div>
          <div class="hero-station" data-tone="recovery"><span></span><div><small>显式恢复</small><strong translate="no">strict replay</strong><code translate="no">resume exact Turn</code></div></div>
        </div>
        <aside class="reading-protocol">
          <span>阅读协议</span>
          <dl><div><dt>内容状态</dt><dd>中文标签区分事实、风险与延期</dd></div><div><dt>示例数据</dt><dd>${renderProse("全部为合成 identity，不对应真实运行")}</dd></div><div><dt>代码定位</dt><dd>${renderProse("路径 + symbol，不固定易漂移行号")}</dd></div><div><dt>核对基准</dt><dd>${renderCode(contextManual.checkedAtCommit)}</dd></div></dl>
        </aside>
      </section>
      <div class="manual-route"><div class="manual-route__rail" aria-hidden="true"></div>${renderKernelPrimer(contextManual.primer, reactMermaid)}${contextManual.chapters.map(renderChapter).join("")}</div>
      <footer class="manual-footer"><div><strong>知识只有一个人工维护源。</strong><p>经确认的领域结论更新 context-site；本 HTML 由构建生成，禁止手改。</p></div><a href="#context/kernel-map">回到核心结构</a></footer>
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
