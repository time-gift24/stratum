"use client"

import { useMemo } from "react"
import { CircleStop } from "lucide-react"

import { ApprovalDock } from "@/components/stratum/conversation/approval-dock"
import type { ApprovalEntry } from "@/components/stratum/conversation/conversation-items"
import { Notice } from "@/components/stratum/conversation/notice"
import {
  AnimatedConversationErrorNotice,
  RealtimeDegradedNotice,
  ResumeNotice,
} from "@/components/stratum/conversation/notices"
import { AgentSelector } from "@/components/stratum/agent-selector"
import { ModelSelector } from "@/components/stratum/model-selector"
import { PromptInput } from "@/components/stratum/prompt-input"
import type { ComposerConfiguration } from "@/hooks/use-agent-conversation"
import type { ConversationState } from "@/features/agent-conversation/types"
import type { ApiError } from "@/lib/stratum/api"
import {
  currentThinkingLevel,
  thinkingLevels,
} from "@/lib/stratum/model-config"

/**
 * ConversationComposer —— /conversation 的 composer 区：审批浮层
 * （ApprovalDock，不推挤消息区）+ 状态提示栈（Notice 族）+ PromptInput
 * （leading = AgentSelector，仅新会话；trailing = ModelSelector）。
 * 模型/Thinking 等级的 schema 驱动派生也收在这里，页面只传 configuration。
 */
export function ConversationComposer({
  configuration,
  pendingApprovals,
  onResolveApproval,
  resumeRequired,
  realtimeDegraded,
  cancelRequested,
  phase,
  error,
  onResume,
  onReconnect,
  onCancel,
  value,
  onChange,
  onSubmit,
}: {
  configuration: ComposerConfiguration
  /** 待决/提交中的审批（ApprovalDock 数据源） */
  pendingApprovals: ApprovalEntry[]
  onResolveApproval: (
    approvalId: string,
    decision: "approve" | "reject"
  ) => void
  resumeRequired: boolean
  realtimeDegraded: boolean
  cancelRequested: boolean
  phase: ConversationState["phase"]
  error: ApiError | null
  onResume: () => void
  onReconnect: () => void
  onCancel: () => void
  /** 受控输入：发送成功才由调用方清空 */
  value: string
  onChange: (value: string) => void
  onSubmit: (value: string) => void
}) {
  const turnRunning = configuration.turnRunning
  const selectedModelConfig = configuration.selectedModelConfig
  const selectedDescriptor = useMemo(
    () =>
      configuration.models.find(
        (descriptor) => descriptor.model === selectedModelConfig?.model
      ),
    [configuration.models, selectedModelConfig?.model]
  )
  const levels = useMemo(
    () => thinkingLevels(selectedDescriptor?.parameters_schema),
    [selectedDescriptor]
  )
  const selectedLevel =
    selectedModelConfig === null
      ? null
      : currentThinkingLevel(selectedModelConfig.parameters)

  return (
    <div className="relative">
      <ApprovalDock
        approvals={pendingApprovals}
        onResolve={onResolveApproval}
      />
      <AnimatedConversationErrorNotice
        phase={phase}
        error={error}
        onReconnect={onReconnect}
      />
      {resumeRequired ||
      realtimeDegraded ||
      (cancelRequested && turnRunning) ? (
        <div className="mb-2 flex flex-col gap-1.5">
          {resumeRequired ? <ResumeNotice onResume={onResume} /> : null}
          {cancelRequested && turnRunning ? (
            <Notice tone="neutral" icon={CircleStop}>
              取消请求已发送。
            </Notice>
          ) : null}
          {realtimeDegraded ? <RealtimeDegradedNotice /> : null}
        </div>
      ) : null}
      <PromptInput
        placeholder="问问 Stratum"
        value={value}
        onChange={onChange}
        onSubmit={onSubmit}
        running={turnRunning && !resumeRequired}
        cancelRequested={cancelRequested}
        onCancel={onCancel}
        leading={
          !configuration.existingRuntime &&
          configuration.agentTemplates.length > 0 ? (
            <AgentSelector
              templates={configuration.agentTemplates}
              selectedTemplate={configuration.selectedTemplate}
              onSelectTemplate={configuration.selectTemplate}
            />
          ) : null
        }
        trailing={
          <div className="flex items-center gap-1.5">
            <ModelSelector
              models={configuration.models}
              selectedModelId={selectedModelConfig?.model ?? null}
              onSelectModel={configuration.selectModel}
              thinkingLevels={levels}
              selectedThinkingLevel={selectedLevel}
              onSelectThinkingLevel={configuration.setThinkingLevel}
              loading={configuration.metadataLoading}
              error={configuration.metadataError !== null}
            />
          </div>
        }
      />
    </div>
  )
}
