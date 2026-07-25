"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { useGSAP } from "@gsap/react"
import { IconArrowDown, IconArrowUp, IconBan } from "@tabler/icons-react"
import gsap from "gsap"
import { useReducedMotion } from "motion/react"
import { useTranslation } from "react-i18next"

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

gsap.registerPlugin(useGSAP)

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
  const reduceMotion = useReducedMotion()
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

  useGSAP(
    () => {
      if (reduceMotion) return
      gsap.fromTo(
        ".chat-entrance",
        { y: 14, opacity: 0, filter: "blur(7px)" },
        {
          y: 0,
          opacity: 1,
          filter: "blur(0px)",
          duration: 0.58,
          stagger: 0.07,
          ease: "power3.out",
          clearProps: "transform,opacity,filter",
        }
      )
    },
    { scope: chatRef, dependencies: [reduceMotion, state.agentId] }
  )

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

  const resolveApproval = async (
    approvalId: string,
    decision: "approve" | "reject"
  ) => {
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
  }

  return (
    <section
      ref={chatRef}
      id="chat"
      className="stratum-chat chat-page__workspace w-full"
      data-conversation-state={isNewConversation ? "new" : "active"}
    >
      <div className="chat-stage px-4 pb-[calc(13rem+env(safe-area-inset-bottom))] sm:px-6 md:px-8 md:pb-[calc(14rem+env(safe-area-inset-bottom))]">
        <div className="stratum-content-width mx-auto">
          <div data-slot="chat-main" className="flex min-w-0 flex-col">
            <div
              ref={messageListRef}
              data-slot="chat-message-list"
              role="log"
              aria-live={state.phase === "recovering" ? "off" : "polite"}
              aria-relevant="additions text"
              className="type-body w-full px-1 py-5 [overflow-anchor:none] sm:px-3 md:px-4 md:py-8"
            >
              <AgentMessageList
                messages={state.messages}
                drafts={state.drafts}
                tools={state.tools}
                approvals={state.approvals}
                approvalSubmissions={approvalSubmissions}
                onApprovalDecision={(approvalId, decision) => {
                  void resolveApproval(approvalId, decision)
                }}
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
          className="stratum-floating-center fixed bottom-[calc(10rem+max(1.75rem,env(safe-area-inset-bottom)))] z-40 size-10 -translate-x-1/2 rounded-full shadow-lg transition-transform duration-200 hover:-translate-y-0.5 motion-reduce:transition-none"
          aria-label={t("chat.scrollToBottom")}
        >
          <IconArrowDown aria-hidden="true" />
        </Button>
      )}

      <div
        data-slot="chat-composer-positioner"
        data-composer-position={isNewConversation ? "centered" : "docked"}
        className="stratum-composer-positioner fixed z-30"
      >
        <div
          data-slot="chat-composer-surface"
          className="stratum-composer-surface chat-entrance"
        >
          <div className="stratum-prompt-shell">
            <PromptInput
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
              <PromptInputFooter className="min-h-12 gap-2 px-2.5 pt-0 pb-[max(0.55rem,env(safe-area-inset-bottom))] sm:px-3.5">
                <PromptInputTools className="[scrollbar-width:none] gap-1.5 overflow-x-auto [&::-webkit-scrollbar]:hidden">
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
                      className="composer-tool h-10 shrink-0 px-3"
                      variant="outline"
                      onClick={() => conversation.reconnect()}
                    >
                      {t("chat.reconnect")}
                    </PromptInputButton>
                  ) : null}
                </PromptInputTools>
                <PromptInputSubmit
                  aria-label={t(
                    canCancel ? "chat.cancel" : "chat.composer.send"
                  )}
                  className="size-11 shrink-0 rounded-lg shadow-md sm:size-10"
                  disabled={
                    !canCancel &&
                    (composerRunning ||
                      composerUnavailable ||
                      composerText.trim() === "")
                  }
                  onClick={
                    canCancel ? () => void conversation.cancel() : undefined
                  }
                  type={canCancel ? "button" : "submit"}
                >
                  {canCancel ? (
                    <IconBan aria-hidden="true" />
                  ) : (
                    <IconArrowUp aria-hidden="true" />
                  )}
                </PromptInputSubmit>
              </PromptInputFooter>
            </PromptInput>
          </div>
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
