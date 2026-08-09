import type { ModelConfig } from "@/lib/stratum/model-config"

/**
 * 一次尚未得到确定成功响应的 message intent。
 *
 * Message API 没有独立 idempotency key；同一 intent 重试必须复用原始
 * expectedCurrentTurnId，才能让 exact-Turn CAS 阻止“已提交但响应丢失”后
 * 再开第二个 Turn。
 */
export type PendingMessageIntent = {
  agentId: string
  text: string
  expectedCurrentTurnId: string | null
  modelConfig?: ModelConfig
}

/**
 * 相同 Agent / 原文 / 完整模型配置表示同一 pending intent，保留旧 CAS；
 * 任一输入变化表示调用方明确形成了新 intent，采用新的 expected CAS。
 */
export function resolveMessageIntent(
  pending: PendingMessageIntent | null,
  next: PendingMessageIntent
): PendingMessageIntent {
  if (
    pending !== null &&
    pending.agentId === next.agentId &&
    pending.text === next.text &&
    sameModelConfig(pending.modelConfig, next.modelConfig)
  )
    return pending
  return next
}

export function sameModelConfig(
  left: ModelConfig | undefined,
  right: ModelConfig | undefined
): boolean {
  if (left === undefined || right === undefined) return left === right
  return (
    left.model === right.model &&
    JSON.stringify(left.parameters) === JSON.stringify(right.parameters)
  )
}
