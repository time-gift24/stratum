import type { ConversationState } from "@/features/agent-conversation/types"
import type { ApiError } from "@/lib/stratum/api"

export type ConversationErrorPresentation = {
  title: string
  description: string
}

/** 将协议错误收敛为面向用户的安全提示，不把后端原文写进会话正文。 */
export function presentConversationError(
  phase: ConversationState["phase"],
  error: ApiError | null
): ConversationErrorPresentation | null {
  if (error === null) return null

  if (phase === "missing" || error.code === "agent_runtime_not_found")
    return {
      title: "会话无法加载",
      description: "该会话可能已删除，或属于另一个 Stratum 运行环境。",
    }

  if (
    error.code === "agent_not_selected" ||
    error.code === "agent_template_not_selected"
  )
    return {
      title: "请先选择 Agent",
      description: "选择一个 Agent 后即可开始对话。",
    }

  if (error.code === "invalid_input")
    return {
      title: "内容无法发送",
      description: "请先输入消息，再重新发送。",
    }

  if (
    error.code === "protocol_identity_error" ||
    error.code === "invalid_response"
  )
    return {
      title: "后端响应不一致",
      description: "为保护当前会话，已停止应用本次响应，请重新连接。",
    }

  if (error.code === "stale_turn")
    return {
      title: "会话状态已经更新",
      description: "同步最新状态后，请重新发送刚才的消息。",
    }

  if (
    error.status === 0 ||
    error.status >= 500 ||
    error.code === "store_unavailable" ||
    error.code === "http_error" ||
    error.code === "command_failed" ||
    error.code === "connection_error"
  )
    return {
      title: "暂时无法连接到 Stratum 后端",
      description: "服务恢复后会自动同步，也可以直接重试刚才的操作。",
    }

  return {
    title: "操作未完成",
    description: "请检查当前会话状态后重试。",
  }
}
