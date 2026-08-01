"use client"

import { useMemo } from "react"

import { ConversationThread } from "@/components/stratum/conversation/conversation-thread"
import { ThreadListRail } from "@/components/stratum/conversation/thread-list-rail"
import type { ConversationMessage } from "@/components/stratum/conversation/types"
import { PromptInput } from "@/components/stratum/prompt-input"
import { useAgentConversation } from "@/hooks/use-agent-conversation"
import { modelDisplayName } from "@/lib/stratum/model-config"

/**
 * DIRECTION CONTRACT —— /conversation 展示页
 * THESIS: conversation 组件库在真实界面里工作——消息流、流式生成、
 *         会话切换，一屏看完；拒绝拆成孤立的 demo 格子。
 * OWN-WORLD: 双 nav 悬浮体系内的完整 chat 界面；消息体走我们自己的
 *            streamdown + Medium 排版（compact），语义色全部 token。
 * STORY: 访客发一条消息，看到真实 agent 的流式回复；左侧切换历史会话。
 * FIRST VIEWPORT: 左侧会话列表 + 右侧消息流 + 底部 PromptInput（电弧激活）。
 * FORM: 整屏 Operate 界面（assistant-ui 底稿的展示层 fork），非 section 展示页。
 *
 * 数据来自 Stratum 后端（REST + SSE）：reasoning / tool calls 留在 state 里，
 * 当前消息模型不渲染。
 */

export default function ConversationPage() {
  const {
    state,
    recentAgents,
    composerConfiguration,
    selectAgent,
    createConversation,
    sendMessage,
  } = useAgentConversation()

  const threads = useMemo(
    () =>
      recentAgents.map((agent) => ({
        id: agent.agentId,
        title: agent.title,
      })),
    [recentAgents]
  )

  const messages = useMemo<ConversationMessage[]>(() => {
    const result: ConversationMessage[] = state.messages
      .filter(
        (message) =>
          (message.role === "user" || message.role === "assistant") &&
          message.text !== null
      )
      .map((message) => ({
        id: `${message.agentId}:${message.messageSeq}`,
        role: message.role as "user" | "assistant",
        content: message.text ?? "",
        status: "done",
      }))

    const draftText = Object.values(state.drafts)
      .map((draft) => draft.text)
      .join("")
    const status = state.view?.status
    if (status === "running") {
      result.push({
        id: "draft",
        role: "assistant",
        content: draftText,
        status: "streaming",
      })
    } else if (status === "failed") {
      result.push({
        id: "draft",
        role: "assistant",
        content: draftText || (state.error?.message ?? "生成失败"),
        status: "error",
      })
    }

    if (state.phase === "connection_error" || state.phase === "missing") {
      result.push({
        id: "connection-error",
        role: "assistant",
        content:
          state.phase === "missing"
            ? "会话不存在或已被删除（404）。"
            : `连接出错：${state.error?.message ?? "无法连接到 Stratum 后端"}`,
        status: "error",
      })
    }

    return result
  }, [state.messages, state.drafts, state.view?.status, state.phase, state.error])

  const modelName = useMemo(() => {
    const config = composerConfiguration.currentModelConfig
    if (config === null)
      return composerConfiguration.metadataLoading
        ? "…"
        : (composerConfiguration.agentName ?? undefined)
    return modelDisplayName(config.model).model
  }, [
    composerConfiguration.currentModelConfig,
    composerConfiguration.agentName,
    composerConfiguration.metadataLoading,
  ])

  const handleSubmit = (value: string) => {
    if (state.agentId === null) void createConversation(value)
    else void sendMessage(value)
  }

  return (
    <div className="flex h-svh pt-24 font-sans sm:pt-28">
      <main className="relative min-w-0 flex-1">
        <ThreadListRail
          threads={threads}
          activeId={state.agentId ?? undefined}
          onSelect={(id) => selectAgent(id)}
          onNew={() => selectAgent(null)}
        />

        <ConversationThread
          messages={messages}
          welcome={
            <h1 className="text-center font-heading text-2xl tracking-tight">
              今天想聊点什么？
            </h1>
          }
          composer={
            <PromptInput
              model={modelName}
              placeholder="问问 Stratum"
              onSubmit={handleSubmit}
            />
          }
        />
      </main>
    </div>
  )
}
