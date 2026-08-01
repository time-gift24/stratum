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
}

export type ConversationThreadMeta = {
  id: string
  title: string
}
