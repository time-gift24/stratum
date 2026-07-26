"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { IconArrowDown, IconArrowUp, IconBan } from "@tabler/icons-react"
import { useTranslation } from "react-i18next"

import BorderGlow from "~/components/BorderGlow"
import {
  AgentConfigMenu,
  ModelConfigMenu,
} from "~/components/stratum/model-config-menu"
import {
  type ApprovalDecision,
  finishApprovalSubmission,
  startApprovalSubmission,
} from "~/components/stratum/agent-approval-submissions"
import { AgentMessageList } from "~/components/stratum/agent-message-list"
import {
  PromptInput,
  PromptInputBody,
  PromptInputButton,
  PromptInputFooter,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
} from "~/components/ai-elements/prompt-input"
import { Button } from "~/components/ui/button"
import type { AgentConversation } from "~/hooks/use-agent-conversation"
import { cn } from "~/lib/utils"

type ChatWorkspaceProps = {
  conversation: AgentConversation
}

type AutoFollowScrollPosition = {
  paused: boolean
  previousScrollTop: number
  scrollTop: number
  targetScrollTop: number
}

const AUTO_FOLLOW_BOTTOM_EPSILON_PX = 1

function resolveAutoFollowPaused({
  paused,
  previousScrollTop,
  scrollTop,
  targetScrollTop,
}: AutoFollowScrollPosition) {
  const atBottom = targetScrollTop - scrollTop <= AUTO_FOLLOW_BOTTOM_EPSILON_PX
  if (paused) {
    return !(atBottom && scrollTop > previousScrollTop)
  }
  return scrollTop < previousScrollTop && !atBottom
}

export function ChatWorkspace({ conversation }: ChatWorkspaceProps) {
  const { t } = useTranslation()
  const [composerText, setComposerText] = useState("")
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [autoFollowPaused, setAutoFollowPaused] = useState(false)
  const [approvalSubmissions, setApprovalSubmissions] = useState<
    ReadonlyMap<string, ApprovalDecision>
  >(() => new Map())
  const composerRef = useRef<HTMLTextAreaElement>(null)
  const messageListRef = useRef<HTMLDivElement>(null)
  const chatRef = useRef<HTMLElement>(null)
  const autoFollowPausedRef = useRef(false)
  const previousScrollTopRef = useRef(0)

  const { state } = conversation
  const isNewConversation = state.agentId === null
  const isAgentBusy =
    state.phase === "recovering" || state.view?.status === "running"
  const composerRunning = isSubmitting || isAgentBusy
  const composerUnavailable = state.phase === "missing"
  const canCancel = state.agentId !== null && state.view?.status === "running"
  const configuration = conversation.composerConfiguration
  const activityAnnouncement = isSubmitting
    ? t(isNewConversation ? "chat.creating" : "chat.sending")
    : state.phase === "recovering"
      ? t("chat.connecting")
      : state.phase === "connection_error"
        ? t("chat.connectionFailed")
        : state.phase === "missing"
          ? t("chat.missingConversation")
          : state.view?.status === "running"
            ? t("chat.thinking")
            : ""

  // 选择对话（包括新建）后聚焦输入框
  useEffect(() => {
    const timer = setTimeout(() => {
      composerRef.current?.focus()
    }, 100)
    return () => clearTimeout(timer)
  }, [state.agentId])

  const scrollToBottom = useCallback((behavior: ScrollBehavior) => {
    if (typeof document === "undefined") return
    const scrollElement = document.documentElement
    scrollElement.scrollTo({
      top: Math.max(scrollElement.scrollHeight - scrollElement.clientHeight, 0),
      behavior,
    })
  }, [])

  const resumeAutoFollow = useCallback(
    (behavior: ScrollBehavior) => {
      autoFollowPausedRef.current = false
      setAutoFollowPaused(false)
      scrollToBottom(behavior)
    },
    [scrollToBottom]
  )
  useEffect(() => {
    if (typeof document === "undefined") return
    const scrollElement = document.documentElement
    previousScrollTopRef.current = scrollElement.scrollTop

    const handleScroll = () => {
      const scrollTop = scrollElement.scrollTop
      const previousScrollTop = previousScrollTopRef.current
      const paused = resolveAutoFollowPaused({
        paused: autoFollowPausedRef.current,
        previousScrollTop,
        scrollTop,
        targetScrollTop: Math.max(
          scrollElement.scrollHeight - scrollElement.clientHeight,
          0
        ),
      })
      previousScrollTopRef.current = scrollTop
      if (paused !== autoFollowPausedRef.current) {
        autoFollowPausedRef.current = paused
        setAutoFollowPaused(paused)
      }
    }

    document.addEventListener("scroll", handleScroll, { passive: true })
    return () => document.removeEventListener("scroll", handleScroll)
  }, [])

  useEffect(() => {
    const messageList = messageListRef.current
    if (!messageList || typeof ResizeObserver === "undefined") return
    let scrollFrame: number | undefined
    const resizeObserver = new ResizeObserver(() => {
      if (autoFollowPausedRef.current) return
      if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame)
      scrollFrame = requestAnimationFrame(() => scrollToBottom("auto"))
    })
    resizeObserver.observe(messageList)
    return () => {
      if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame)
      resizeObserver.disconnect()
    }
  }, [scrollToBottom])

  useEffect(() => {
    resumeAutoFollow("auto")
  }, [resumeAutoFollow, state.agentId])

  const submitMessage = async () => {
    const text = composerText.trim()
    if (text === "" || isSubmitting || isAgentBusy) return

    setIsSubmitting(true)
    try {
      const sent =
        state.agentId === null
          ? await conversation.createConversation(text)
          : await conversation.sendMessage(text)
      if (sent) setComposerText("")
    } finally {
      setIsSubmitting(false)
    }
  }

  const resolveApproval = useCallback(
    async (approvalId: string, decision: "approve" | "reject") => {
      setApprovalSubmissions((submissions) =>
        startApprovalSubmission(submissions, approvalId, decision)
      )
      try {
        await conversation.resolveApproval(approvalId, decision)
      } finally {
        setApprovalSubmissions((submissions) =>
          finishApprovalSubmission(submissions, approvalId)
        )
      }
    },
    [conversation]
  )

  const renderComposerFooter = (
    footerClassName: string,
    toolsClassName: string
  ) => (
    <PromptInputFooter className={footerClassName}>
      <PromptInputTools className={toolsClassName}>
        <AgentConfigMenu
          configuration={configuration}
          commandPending={isSubmitting}
        />
        <ModelConfigMenu
          configuration={configuration}
          commandPending={isSubmitting}
        />
        {state.phase === "connection_error" ? (
          <PromptInputButton
            className="h-10 shrink-0 rounded-lg border-transparent bg-transparent px-3 text-muted-foreground hover:bg-secondary hover:text-foreground"
            variant="outline"
            onClick={() => conversation.reconnect()}
          >
            {t("chat.reconnect")}
          </PromptInputButton>
        ) : null}
      </PromptInputTools>
      <PromptInputSubmit
        aria-label={t(canCancel ? "chat.cancel" : "chat.composer.send")}
        className="size-11 shrink-0 rounded-lg shadow-md sm:size-10"
        disabled={
          !canCancel &&
          (composerRunning || composerUnavailable || composerText.trim() === "")
        }
        onClick={canCancel ? () => void conversation.cancel() : undefined}
        type={canCancel ? "button" : "submit"}
      >
        {canCancel ? (
          <IconBan aria-hidden="true" />
        ) : (
          <IconArrowUp aria-hidden="true" />
        )}
      </PromptInputSubmit>
    </PromptInputFooter>
  )

  const renderComposerInput = (inputGroupClassName: string) => (
    <PromptInput
      className={cn(
        "relative z-10 border-0 bg-transparent shadow-none [&_[data-slot=input-group]]:border-0",
        inputGroupClassName
      )}
      aria-busy={composerRunning}
      onSubmit={(event) => {
        event.preventDefault()
        void submitMessage()
      }}
    >
      <PromptInputBody>
        <PromptInputTextarea
          ref={composerRef}
          aria-label={t("chat.composer.label")}
          className="max-h-56 min-h-24 px-4 pt-4 pb-3 text-base! leading-7! placeholder:text-muted-foreground md:px-5"
          disabled={composerRunning || composerUnavailable}
          onChange={(event) => setComposerText(event.target.value)}
          placeholder={t("chat.composer.placeholder")}
          value={composerText}
        />
      </PromptInputBody>
      {renderComposerFooter(
        "grid min-h-12 grid-cols-[minmax(0,1fr)_auto] [gap:calc(0.5rem*var(--p-density,1))] px-[calc(0.75rem*var(--p-density,1))] pt-0 pb-[max(0.55rem,env(safe-area-inset-bottom))]",
        "[scrollbar-width:none] gap-1.5 overflow-x-auto [&::-webkit-scrollbar]:hidden [&_[data-tone]]:text-[0.8125rem] [&_[data-tone=agent]]:bg-primary/8 [&_[data-tone=agent]]:text-primary [&_[data-tone=model]]:bg-chart-1/8 [&_[data-tone=model]]:text-chart-1 [&_[data-tone=thinking]]:bg-chart-2/8 [&_[data-tone=thinking]]:text-chart-2"
      )}
    </PromptInput>
  )

  return (
    <section
      ref={chatRef}
      id="chat"
      className="relative isolate min-h-[calc(100dvh-var(--global-nav-offset))] w-full"
      data-conversation-state={isNewConversation ? "new" : "active"}
    >
      <div className="min-h-[calc(100dvh-var(--global-nav-offset))] px-4 pb-[calc(13rem+env(safe-area-inset-bottom))] sm:px-6 md:px-8 md:pb-[calc(14rem+env(safe-area-inset-bottom))]">
        <div className="mx-auto w-full max-w-(--content-width)">
          <div data-slot="chat-main" className="flex min-w-0 flex-col">
            <div
              ref={messageListRef}
              data-slot="chat-message-list"
              role="log"
              aria-live={state.phase === "recovering" ? "off" : "polite"}
              aria-relevant="additions text"
              className="w-full px-1 py-5 text-[0.9375rem]/[1.65] [overflow-anchor:none] sm:px-3 md:px-4 md:py-8"
            >
              <AgentMessageList
                messages={state.messages}
                drafts={state.drafts}
                tools={state.tools}
                approvals={state.approvals}
                approvalSubmissions={approvalSubmissions}
                onApprovalDecision={resolveApproval}
                error={state.error}
              />
            </div>
          </div>
        </div>
      </div>

      {autoFollowPaused && (
        <Button
          type="button"
          size="icon"
          variant="outline"
          onClick={() => resumeAutoFollow("smooth")}
          className="fixed bottom-[calc(10rem+max(1.75rem,env(safe-area-inset-bottom)))] left-1/2 z-40 size-10 -translate-x-1/2 rounded-full shadow-lg transition-transform duration-200 hover:-translate-y-0.5 motion-reduce:transition-none"
          aria-label={t("chat.scrollToBottom")}
        >
          <IconArrowDown aria-hidden="true" />
        </Button>
      )}

      <div
        data-slot="chat-composer-positioner"
        data-composer-position={isNewConversation ? "centered" : "docked"}
        className={cn(
          "fixed right-[max(1rem,env(safe-area-inset-right))] left-[max(1rem,env(safe-area-inset-left))] z-(--z-composer) mx-auto w-auto max-w-(--composer-width) transition-[bottom] duration-300 ease-(--ease-interface)",
          isNewConversation
            ? "bottom-[46%] max-sm:bottom-[43%]"
            : "bottom-[max(1rem,env(safe-area-inset-bottom))]"
        )}
      >
        <div
          data-slot="chat-composer-surface"
          className={cn(
            "transition-transform duration-300 ease-(--ease-interface)",
            isNewConversation && "translate-y-1/2"
          )}
        >
          <BorderGlow
            backgroundColor="var(--card)"
            className="w-full"
            borderRadius="var(--radius-xl)"
            colors={["var(--primary)", "var(--chart-2)", "var(--chart-1)"]}
            fillOpacity={0}
          >
            {renderComposerInput(
              "[&_[data-slot=input-group]]:bg-card/66 [&_[data-slot=input-group]]:shadow-[0_28px_78px_color-mix(in_srgb,var(--background)_76%,transparent)] [&_[data-slot=input-group]]:backdrop-blur-2xl"
            )}
          </BorderGlow>
          {activityAnnouncement ? (
            <p className="sr-only" role="status" aria-live="polite">
              {activityAnnouncement}
            </p>
          ) : null}
        </div>
      </div>
    </section>
  )
}
