"use client"

import {
  useEffect,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react"
import { IconClock, IconHistory, IconTrash, IconX } from "@tabler/icons-react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import { Link } from "react-router"
import { useTranslation } from "react-i18next"

import { glassSurface } from "~/components/stratum/glass-surface"
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
  onClose(): void
  onRemoveAgent(agentId: string): void
}

function ConversationCard({
  agent,
  active,
  missing,
  language,
  removeLabel,
  onClose,
  onRemoveAgent,
}: ConversationCardProps) {
  return (
    <li className="group flex items-center gap-1">
      <Link
        to={`/chat?agent=${encodeURIComponent(agent.agentId)}`}
        onClick={onClose}
        aria-current={active ? "page" : undefined}
        className={cn(
          "flex min-h-14 min-w-0 flex-1 items-center gap-3 rounded-lg px-2.5 text-sm transition-[background-color,color,box-shadow,transform] duration-200 ease-out focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
          active
            ? "bg-foreground/7 text-foreground shadow-inner"
            : "bg-secondary/45 text-muted-foreground shadow-lg shadow-background/15 hover:-translate-y-0.5 hover:bg-secondary/75 hover:text-foreground",
          missing && "text-destructive"
        )}
      >
        <span
          className={cn(
            "grid shrink-0 place-items-center rounded-md transition-colors",
            active
              ? "size-10 bg-primary/15 text-primary"
              : "size-8 bg-secondary/75 text-muted-foreground group-hover:text-foreground"
          )}
        >
          <IconClock className="size-4" aria-hidden="true" />
        </span>
        <span className="min-w-0 flex-1 truncate font-medium">
          {agent.title}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">
          {formatRelativeTime(agent.lastOpenedAt, language)}
        </span>
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
  onClose,
  onRemoveAgent,
  language,
  emptyLabel,
  removeLabel,
}: Omit<HistoryPanelProps, "open"> & {
  language: string
  emptyLabel: string
  removeLabel: string
}) {
  const activeAgent = activeAgentId
    ? recentAgents.find((agent) => agent.agentId === activeAgentId)
    : undefined
  const orderedAgents = activeAgent
    ? [
        activeAgent,
        ...recentAgents.filter(
          (agent) => agent.agentId !== activeAgent.agentId
        ),
      ]
    : recentAgents

  return (
    <div
      className={cn(
        glassSurface({ surface: "card", elevation: "inset" }),
        "mx-2 mb-2 flex min-h-0 flex-1 flex-col rounded-xl p-2"
      )}
    >
      {orderedAgents.length === 0 ? (
        <div className="grid flex-1 place-items-center px-6 text-center text-sm text-muted-foreground">
          {emptyLabel}
        </div>
      ) : (
        <ul className="flex min-h-0 flex-1 [scrollbar-width:thin] flex-col gap-2 overflow-y-auto">
          {orderedAgents.map((agent) => (
            <ConversationCard
              key={agent.agentId}
              agent={agent}
              active={agent.agentId === activeAgentId}
              missing={agent.agentId === missingAgentId}
              language={language}
              removeLabel={removeLabel}
              onClose={onClose}
              onRemoveAgent={onRemoveAgent}
            />
          ))}
        </ul>
      )}
    </div>
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
  const panelRef = useRef<HTMLElement>(null)
  const closeButtonRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!open) return
    const focusFrame = requestAnimationFrame(() =>
      closeButtonRef.current?.focus()
    )
    return () => cancelAnimationFrame(focusFrame)
  }, [open])

  const keepFocusInside = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab") return
    const panel = panelRef.current
    if (!panel) return
    const focusable = Array.from(
      panel.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])'
      )
    )
    const first = focusable[0]
    const last = focusable.at(-1)
    if (!first || !last) return

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    } else if (!panel.contains(document.activeElement)) {
      event.preventDefault()
      first.focus()
    }
  }

  return (
    <AnimatePresence initial={false}>
      {open ? (
        <>
          <motion.button
            type="button"
            aria-label={t("chat.history.close")}
            className="fixed inset-0 z-(--z-overlay) cursor-default bg-transparent"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.18 }}
            onClick={onClose}
          />
          <motion.aside
            ref={panelRef}
            className={cn(
              glassSurface({ surface: "popover", elevation: "overlay" }),
              "fixed top-(--global-nav-offset) bottom-3 left-24 z-(--z-modal) flex w-84 flex-col rounded-xl max-sm:right-2 max-sm:bottom-2 max-sm:left-19 max-sm:w-auto"
            )}
            role="dialog"
            aria-modal="true"
            aria-label={t("productShell.recent")}
            onKeyDown={keepFocusInside}
            initial={{
              x: reduceMotion ? 0 : -18,
              y: reduceMotion ? 0 : 8,
              scale: reduceMotion ? 1 : 0.97,
              opacity: 0,
            }}
            animate={{ x: 0, y: 0, scale: 1, opacity: 1 }}
            exit={{
              x: reduceMotion ? 0 : -12,
              y: reduceMotion ? 0 : 5,
              scale: reduceMotion ? 1 : 0.985,
              opacity: 0,
            }}
            transition={{
              duration: reduceMotion ? 0 : 0.24,
              ease: [0.22, 1, 0.36, 1],
            }}
          >
            <div className="flex h-14 shrink-0 items-center justify-between px-4">
              <div className="flex items-center gap-2.5">
                <IconHistory
                  className="size-4 text-muted-foreground"
                  aria-hidden="true"
                />
                <h2 className="font-heading text-sm font-medium text-foreground">
                  {t("productShell.recent")}
                </h2>
              </div>
              <Button
                ref={closeButtonRef}
                type="button"
                variant="ghost"
                size="icon"
                className="size-10"
                onClick={onClose}
                aria-label={t("chat.history.close")}
              >
                <IconX aria-hidden="true" />
              </Button>
            </div>
            <ConversationList
              recentAgents={recentAgents}
              activeAgentId={activeAgentId}
              missingAgentId={missingAgentId}
              language={language}
              emptyLabel={t("productShell.noRecent")}
              removeLabel={t("chat.removeLocalEntry")}
              onClose={onClose}
              onRemoveAgent={onRemoveAgent}
            />
          </motion.aside>
        </>
      ) : null}
    </AnimatePresence>
  )
}
