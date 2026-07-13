import type { ReactNode } from "react"
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { BrainIcon, ChevronDownIcon } from "lucide-react"

import {
  Message,
  MessageContent,
  MessageResponse,
} from "~/components/ai-elements/message"
import {
  Reasoning,
  ReasoningContent,
} from "~/components/ai-elements/reasoning"
import { Shimmer } from "~/components/ai-elements/shimmer"
import {
  Tool,
  ToolContent,
  ToolHeader,
  type ToolStatus,
} from "~/components/ai-elements/tool"
import { CodeBlock } from "~/components/ai-elements/code-block"
import type {
  StableMessage,
  ToolProgress,
} from "~/features/agent-conversation/types"
import type { ApiError } from "~/lib/wyse-api"
import { cn } from "~/lib/utils"

type AgentMessageListProps = {
  messages: readonly StableMessage[]
  drafts: Readonly<Record<string, { text: string; reasoning: string }>>
  tools: Readonly<Record<string, ToolProgress>>
  error?: ApiError | null
}

type ReasoningDisclosureProps = {
  children: string
  isStreaming?: boolean
  getThinkingMessage(isStreaming: boolean): ReactNode
}

function ReasoningDisclosure({
  children,
  isStreaming = false,
  getThinkingMessage,
}: ReasoningDisclosureProps) {
  const [open, setOpen] = useState(false)

  return (
    <Reasoning
      defaultOpen={false}
      isStreaming={isStreaming}
      onOpenChange={setOpen}
      open={open}
    >
      <button
        aria-expanded={open}
        className="flex w-full items-center gap-2 text-left text-sm text-muted-foreground transition-colors hover:text-foreground"
        onClick={() => setOpen((currentOpen) => !currentOpen)}
        type="button"
      >
        <BrainIcon className="size-4" />
        {getThinkingMessage(isStreaming)}
        <ChevronDownIcon
          className={cn(
            "size-4 transition-transform",
            open ? "rotate-180" : "rotate-0"
          )}
        />
      </button>
      <ReasoningContent>{children}</ReasoningContent>
    </Reasoning>
  )
}

function toToolStatus(status: ToolProgress["status"]): ToolStatus {
  switch (status) {
    case "streaming":
      return "running"
    case "finished":
      return "completed"
    case "failed":
      return "error"
    default:
      return "pending"
  }
}

export function AgentMessageList({
  messages,
  drafts,
  tools,
  error = null,
}: AgentMessageListProps) {
  const { t, i18n } = useTranslation()
  const dateTimeFormat = new Intl.DateTimeFormat(i18n.resolvedLanguage, {
    dateStyle: "short",
    timeStyle: "short",
  })
  const thinkingMessage = (isStreaming: boolean) =>
    isStreaming ? (
      <Shimmer as="span" duration={1.4}>
        {t("chat.thinking")}
      </Shimmer>
    ) : (
      t("chat.reasoningComplete")
    )

  return (
    <>
      {messages.map((message) => {
        const isUser = message.role === "user"
        const text = message.text ?? JSON.stringify(message.json)

        return (
          <div
            key={`${message.agentId}:${message.businessSeq}`}
            className="animate-in duration-200 fade-in-0 slide-in-from-bottom-2"
          >
            <Message from={isUser ? "user" : "assistant"}>
              {message.reasoning ? (
                <ReasoningDisclosure getThinkingMessage={thinkingMessage}>
                  {message.reasoning}
                </ReasoningDisclosure>
              ) : null}
              <MessageContent>
                <MessageResponse>{text}</MessageResponse>
              </MessageContent>
              <time
                dateTime={message.timestamp}
                className={
                  isUser
                    ? "self-end px-1 text-[0.625rem] text-muted-foreground"
                    : "px-1 text-[0.625rem] text-muted-foreground"
                }
              >
                {dateTimeFormat.format(new Date(message.timestamp))}
              </time>
            </Message>
          </div>
        )
      })}

      {Object.entries(drafts).map(([callId, draft]) => (
        <div
          key={callId}
          className="animate-in duration-200 fade-in-0 slide-in-from-bottom-2"
        >
          <Message from="assistant">
            {draft.reasoning ? (
              <ReasoningDisclosure
                isStreaming
                getThinkingMessage={thinkingMessage}
              >
                {draft.reasoning}
              </ReasoningDisclosure>
            ) : null}
            <MessageContent>
              <MessageResponse>{draft.text}</MessageResponse>
            </MessageContent>
          </Message>
        </div>
      ))}

      {Object.values(tools).length > 0 ? (
        <div>
          <div className="flex flex-col gap-2">
            {Object.values(tools).map((tool) => (
              <Tool key={tool.callId} defaultOpen={tool.status === "streaming"}>
                <ToolHeader
                  status={toToolStatus(tool.status)}
                  statusLabel={t(`chat.toolStatus.${tool.status}`)}
                  title={tool.name ?? t("chat.unknownTool")}
                />
                <ToolContent>
                  {tool.argumentsText ? (
                    <CodeBlock code={tool.argumentsText} language="json" />
                  ) : null}
                  {tool.result ? (
                    <CodeBlock
                      code={JSON.stringify(tool.result, null, 2)}
                      language="json"
                    />
                  ) : null}
                  {tool.errorText ? <p>{tool.errorText}</p> : null}
                </ToolContent>
              </Tool>
            ))}
          </div>
        </div>
      ) : null}

      {error ? (
        <div className="animate-in duration-200 fade-in-0 slide-in-from-bottom-2">
          <Message from="assistant">
            <MessageContent>
              <div className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
                <p className="font-medium">{t("chat.connectionFailed")}</p>
                {error.message ? (
                  <p className="mt-1 text-destructive/80">{error.message}</p>
                ) : null}
              </div>
            </MessageContent>
          </Message>
        </div>
      ) : null}
    </>
  )
}
