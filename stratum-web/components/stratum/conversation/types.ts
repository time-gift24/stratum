/**
 * Conversation 组件库的数据模型（数据驱动，无 runtime）。
 * assistant-ui 底稿的 primitives/provider 全部剥掉，状态由调用方持有。
 */

export type ConversationMessage = {
  id: string
  role: "user" | "assistant"
  /** markdown 正文。助手消息经 streamdown 渲染，用户消息按纯文本展示 */
  content: string
  /**
   * 同一消息的候选版本（assistant-ui 的 branch）。长度 >1 时显示分支切换，
   * 正文展示 versions[activeIndex] 而非 content。
   */
  versions?: string[]
  /** streaming：正文仍在生成（渲染 caret + streaming 模式） */
  status?: "streaming" | "done" | "error"
  /** 助手消息的思考过程（纯文本）；为空时不渲染 Reasoning 块 */
  reasoning?: string
  /** reasoning 非 streaming 时的默认视图：本轮新消息 "preview"，历史消息 "collapsed" */
  reasoningDefaultView?: "collapsed" | "preview"
  /** 助手消息的工具调用（含挂接的审批）；为空时不渲染工具块 */
  toolCalls?: ConversationToolCall[]
}

export type ConversationThreadMeta = {
  id: string
  title: string
}

/**
 * 消息列条目：普通消息 + TranscriptCompacted 可折叠 marker + 安全
 * terminal marker（failed/cancelled）。id = `${agentId}:${eventSeq}`。
 */
export type ConversationItem =
  | { kind: "message"; id: string; message: ConversationMessage }
  | {
      kind: "compaction"
      id: string
      /** 完整 summary（展开可见；原消息仍在更早分页中保留） */
      summary: string
      compactedIteration: number
    }
  | {
      kind: "terminal"
      id: string
      terminal: "failed" | "cancelled"
      errorText: string | null
    }

/** 工具审批视图（页面由 state.approvals + 本会话已决结果组装） */
export type ToolCallApproval = {
  approvalId: string
  callId: string
  toolName: string
  toolKind: "read" | "write"
  dangerLevel: "low" | "medium" | "high"
  /** pending=待决；submitting=已点击、等待后端确认；approved/rejected=本会话已决终态 */
  status: "pending" | "submitting" | "approved" | "rejected"
}

/** 工具调用视图（实时 ToolProgress 或历史 StableMessage.toolCalls 的统一渲染模型） */
export type ConversationToolCall = {
  callId: string
  name: string | null
  /** arguments 的可显示文本（JSON）；空串表示无参数 */
  argumentsText: string
  /** result 的可显示文本；null 表示尚无结果 */
  result: string | null
  errorText: string | null
  /** interrupted：Turn terminal 时仍无 durable result（不伪造结果） */
  status: "streaming" | "finished" | "failed" | "interrupted"
  /** 挂接到该调用的审批（按 callId 配对） */
  approval?: ToolCallApproval
}
