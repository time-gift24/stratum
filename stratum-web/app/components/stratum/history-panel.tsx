"use client"

import { useEffect, useRef } from "react"
import { IconClock, IconTrash, IconX } from "@tabler/icons-react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import { Link } from "react-router"
import { useTranslation } from "react-i18next"

import {
  FeatureCard,
  FeatureCardContent,
} from "~/components/stratum/feature-card"
import { Button } from "~/components/ui/button"
import { formatRelativeTime, type RecentAgent } from "~/lib/recent-agents"
import { cn } from "~/lib/utils"

type HistoryPanelProps = {
  open: boolean
  onClose(): void
  activeAgentId: string | null
  missingAgentId: string | null
  recentAgents: readonly RecentAgent[]
  onRemoveAgent(agentId: string): void
}

type ConversationCardProps = {
  agent: RecentAgent
  active: boolean
  missing: boolean
  language: string
  removeLabel: string
  onRemoveAgent(agentId: string): void
}

type ConversationIdentityProps = {
  agent: RecentAgent
  active?: boolean
  language: string
}

function ConversationIdentity({
  agent,
  active = false,
  language,
}: ConversationIdentityProps) {
  return (
    <>
      <span
        className={cn(
          "grid size-7 shrink-0 place-items-center transition-colors",
          active
            ? "text-primary"
            : "text-muted-foreground group-hover:text-foreground"
        )}
      >
        <IconClock className="size-4" aria-hidden="true" />
      </span>
      <span className="min-w-0 flex-1 truncate font-medium">{agent.title}</span>
      <span className="shrink-0 text-xs text-muted-foreground">
        {formatRelativeTime(agent.lastOpenedAt, language)}
      </span>
    </>
  )
}

function ConversationCard({
  agent,
  active,
  missing,
  language,
  removeLabel,
  onRemoveAgent,
}: ConversationCardProps) {
  return (
    <li className="group flex items-center gap-1 py-0.5">
      <Link
        to={`/chat?agent=${encodeURIComponent(agent.agentId)}`}
        aria-current={active ? "page" : undefined}
        className={cn(
          "relative flex min-h-11 min-w-0 flex-1 items-center gap-2.5 rounded-md px-3 text-sm transition-colors duration-200 ease-out before:absolute before:top-1/2 before:left-0 before:h-5 before:w-px before:-translate-y-1/2 before:rounded-full before:bg-transparent before:transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-hidden",
          active
            ? "text-foreground before:bg-primary"
            : "text-muted-foreground hover:text-foreground",
          missing && "text-destructive"
        )}
      >
        <ConversationIdentity
          agent={agent}
          active={active}
          language={language}
        />
      </Link>
      {missing ? (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-11 shrink-0 text-destructive"
          onClick={() => onRemoveAgent(agent.agentId)}
          aria-label={removeLabel}
        >
          <IconTrash aria-hidden="true" />
        </Button>
      ) : null}
    </li>
  )
}

function ConversationList({
  activeAgentId,
  missingAgentId,
  recentAgents,
  onRemoveAgent,
  language,
  emptyLabel,
  removeLabel,
}: Omit<HistoryPanelProps, "open" | "onClose"> & {
  language: string
  emptyLabel: string
  removeLabel: string
}) {
  return (
    <FeatureCardContent className="p-2">
      {recentAgents.length === 0 ? (
        <div className="grid min-h-16 place-items-center px-5 text-center text-sm text-muted-foreground">
          {emptyLabel}
        </div>
      ) : (
        <ul className="flex max-h-[calc(100dvh-var(--global-nav-offset)-5rem)] [scrollbar-width:thin] flex-col divide-y divide-border/25 overflow-y-auto">
          {recentAgents.map((agent) => (
            <ConversationCard
              key={agent.agentId}
              agent={agent}
              active={agent.agentId === activeAgentId}
              missing={agent.agentId === missingAgentId}
              language={language}
              removeLabel={removeLabel}
              onRemoveAgent={onRemoveAgent}
            />
          ))}
        </ul>
      )}
    </FeatureCardContent>
  )
}

export function HistoryPanel({
  open,
  onClose,
  activeAgentId,
  missingAgentId,
  recentAgents,
  onRemoveAgent,
}: HistoryPanelProps) {
  const { t, i18n } = useTranslation()
  const reduceMotion = useReducedMotion()
  const language = i18n.resolvedLanguage ?? "en"
  const closeButtonRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!open) return
    const focusFrame = requestAnimationFrame(() =>
      closeButtonRef.current?.focus()
    )
    return () => cancelAnimationFrame(focusFrame)
  }, [open])

  return (
    <AnimatePresence initial={false}>
      {open ? (
        <motion.aside
          id="history-panel"
          className="w-full self-start"
          aria-label={t("productShell.recent")}
          initial={{
            x: reduceMotion ? 0 : -12,
            opacity: 0,
          }}
          animate={{ x: 0, opacity: 1 }}
          exit={{
            x: reduceMotion ? 0 : -8,
            opacity: 0,
          }}
          transition={{
            duration: reduceMotion ? 0 : 0.22,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <FeatureCard>
            <div className="flex h-9 shrink-0 items-center justify-between px-2">
              <div className="flex items-center gap-2">
                <span
                  aria-hidden="true"
                  className="size-2 rounded-full bg-foreground shadow-[0_0_10px_color-mix(in_srgb,var(--foreground)_50%,transparent)]"
                />
                <h2 className="font-heading text-[0.8125rem] font-medium text-foreground">
                  {t("productShell.recent")}
                </h2>
              </div>
              <Button
                ref={closeButtonRef}
                type="button"
                variant="ghost"
                size="icon"
                className="size-8 rounded-md"
                onClick={onClose}
                aria-label={t("chat.history.close")}
              >
                <IconX className="size-3.5" aria-hidden="true" />
              </Button>
            </div>
            <ConversationList
              recentAgents={recentAgents}
              activeAgentId={activeAgentId}
              missingAgentId={missingAgentId}
              language={language}
              emptyLabel={t("productShell.noRecent")}
              removeLabel={t("chat.removeLocalEntry")}
              onRemoveAgent={onRemoveAgent}
            />
          </FeatureCard>
        </motion.aside>
      ) : null}
    </AnimatePresence>
  )
}
