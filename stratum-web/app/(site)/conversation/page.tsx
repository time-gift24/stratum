"use client"

import { useCallback, useMemo, useState } from "react"

import { ConversationComposer } from "@/components/stratum/conversation/conversation-composer"
import {
  buildSettledItems,
  composeLiveItems,
} from "@/components/stratum/conversation/conversation-items"
import { ConversationThread } from "@/components/stratum/conversation/conversation-thread"
import { ThreadListRail } from "@/components/stratum/conversation/thread-list-rail"
import { useApprovalViews } from "@/components/stratum/conversation/use-approval-views"
import { useAgentConversation } from "@/hooks/use-agent-conversation"

/**
 * DIRECTION CONTRACT —— /conversation 展示页
 * THESIS: conversation 组件库在真实界面里工作——消息流、流式生成、
 *         会话切换，一屏看完；拒绝拆成孤立的 demo 格子。
 * OWN-WORLD: 双 nav 悬浮体系内的完整 chat 界面；消息体走我们自己的
 *            streamdown + Medium 排版（compact），语义色全部 token。
 * STORY: 访客发一条消息，看到真实 agent 的流式回复；左侧切换历史会话。
 * FIRST VIEWPORT: 左侧会话列表 + 右侧消息流 + 底部 PromptInput（模型选择 + 电弧激活）。
 * FORM: 整屏 Operate 界面（assistant-ui 底稿的展示层 fork），非 section 展示页。
 *
 * 数据来自 Stratum 后端（Postgres-first REST + AgentRuntime-scoped SSE）：durable
 * identity 是 (agentRuntimeId, eventSeq 十进制字符串)；reasoning 与 tool calls 在
 * 正文上方渐进式透明展示（默认折叠，待决审批强制展开可操作）；
 * TranscriptCompacted 渲染为可折叠"上下文已压缩" marker；failed/cancelled
 * 渲染为安全 terminal marker；向上滚动按固定 through barrier 分页更旧历史。
 *
 * 本页只是编排层：state → 视图条目的映射在 conversation-items.ts（纯函数），
 * 审批状态派生在 use-approval-views.ts，composer 区在 conversation-composer.tsx。
 */

const WELCOME = (
  <h1 className="text-center font-heading text-2xl tracking-tight">
    今天想聊点什么？
  </h1>
)

export default function ConversationPage() {
  const {
    state,
    recentAgentRuntimes,
    composerConfiguration,
    selectAgentRuntime,
    createConversation,
    sendMessage,
    cancel,
    resume,
    reconnect,
    resolveApproval,
    loadOlderHistory,
  } = useAgentConversation()

  const threads = useMemo(
    () =>
      recentAgentRuntimes.map((runtime) => ({
        id: runtime.agentRuntimeId,
        title: runtime.title,
      })),
    [recentAgentRuntimes]
  )

  // 历史/新消息区分：recovery 完成（ready）时快照当时的 barrier；seq ≤ 该
  // barrier 的消息为历史（reasoning 默认折叠，含之后向上分页加载的旧页），
  // 之后到达的为本轮新消息（默认简略预览）。
  // derive-state-during-render 模式：渲染期条件 setState，立即重渲染提交。
  const [historical, setHistorical] = useState<{
    agentRuntimeId: string | null
    historyThrough: string | null
    pgConfirmedEventSeq: string
  }>({
    agentRuntimeId: null,
    historyThrough: null,
    pgConfirmedEventSeq: "0",
  })
  if (
    state.phase === "ready" &&
    (historical.agentRuntimeId !== state.agentRuntimeId ||
      historical.historyThrough !== state.historyThrough)
  ) {
    setHistorical({
      agentRuntimeId: state.agentRuntimeId,
      historyThrough: state.historyThrough,
      pgConfirmedEventSeq: state.pgConfirmedEventSeq,
    })
  }

  const approvals = useApprovalViews(
    state.approvals,
    state.agentRuntimeId,
    resolveApproval
  )

  // 冷段（timeline 落成部分）+ 热段（draft/实时 tools）；运行错误统一在
  // composer 上方的 Notice 展示，绝不伪装成 assistant 消息污染正文。
  // 流式 token 每帧只重跑热段，settled 视图经 WeakMap 缓存复用引用
  const settled = useMemo(
    () =>
      buildSettledItems(
        state.timeline,
        state.tools,
        approvals.entries,
        historical.pgConfirmedEventSeq
      ),
    [state.timeline, state.tools, approvals.entries, historical]
  )
  const items = useMemo(
    () =>
      composeLiveItems(
        settled,
        state.drafts,
        state.tools,
        state.view?.status,
        approvals.entries
      ),
    [settled, state.drafts, state.tools, state.view?.status, approvals.entries]
  )

  const [sendVersion, setSendVersion] = useState(0)
  // 受控 composer：发送成功才清空；首条消息失败等场景保留用户原文
  const [composerValue, setComposerValue] = useState("")
  const handleSubmit = (value: string) => {
    // 发送信号：让 thread 把随后的 null → 新 runtime id 识别为同一对话的首发
    setSendVersion((version) => version + 1)
    const sent =
      state.agentRuntimeId === null
        ? createConversation(value)
        : sendMessage(value)
    void sent.then((ok) => {
      // 请求期间用户可能已经继续输入；只清掉本次实际发送的原值。
      if (ok) setComposerValue((current) => (current === value ? "" : current))
    })
  }

  const handleNewConversation = useCallback(
    () => selectAgentRuntime(null),
    [selectAgentRuntime]
  )

  return (
    <div className="flex h-svh pt-20 font-sans">
      <main className="relative min-w-0 flex-1">
        <ThreadListRail
          threads={threads}
          activeId={state.agentRuntimeId ?? undefined}
          onSelect={selectAgentRuntime}
          onNew={handleNewConversation}
        />

        <ConversationThread
          items={items}
          conversationId={state.agentRuntimeId}
          sendVersion={sendVersion}
          recovering={state.phase === "recovering"}
          hasOlder={state.historyHasMore}
          olderLoading={state.historyLoading}
          onLoadOlder={loadOlderHistory}
          welcome={WELCOME}
          composer={
            <ConversationComposer
              configuration={composerConfiguration}
              pendingApprovals={approvals.pending}
              onResolveApproval={approvals.resolve}
              resumeRequired={state.view?.resume_required === true}
              realtimeDegraded={state.realtimeDegraded}
              cancelRequested={state.cancelRequested}
              phase={state.phase}
              error={state.error}
              onResume={() => void resume()}
              onReconnect={reconnect}
              onCancel={() => void cancel()}
              value={composerValue}
              onChange={setComposerValue}
              onSubmit={handleSubmit}
            />
          }
        />
      </main>
    </div>
  )
}
