"use client"

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react"
import { IconClock, IconHistory, IconTrash, IconX } from "@tabler/icons-react"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import { Link, useLocation, useNavigate } from "react-router"
import { useTranslation } from "react-i18next"

import { Button } from "~/components/ui/button"
import type { AgentTemplateView, ModelDescriptor } from "~/lib/model-config"
import {
  formatRelativeTime,
  loadRecentAgents,
  rememberRecentAgent as rememberStoredRecentAgent,
  removeRecentAgent as removeStoredRecentAgent,
  type RecentAgent,
  type StorageLike,
} from "~/lib/recent-agents"
import {
  ApiError,
  createStratumApi,
  STRATUM_API_BASE_URL,
} from "~/lib/stratum-api"
import { cn } from "~/lib/utils"

type ResourcePhase = "loading" | "loaded" | "empty" | "error"

export type WorkbenchResource<T> = {
  items: readonly T[]
  phase: ResourcePhase
  error: ApiError | null
}

type ProductWorkbenchContextValue = {
  templates: WorkbenchResource<AgentTemplateView>
  models: WorkbenchResource<ModelDescriptor>
  recentAgents: readonly RecentAgent[]
  activeAgentId: string | null
  missingAgentId: string | null
  metadataLoading: boolean
  metadataError: ApiError | null
  refreshTemplates(): Promise<void>
  refreshModels(): Promise<void>
  openHistory(): void
  rememberRecentAgent(agent: RecentAgent): void
  removeRecentAgent(agentId: string): void
  setActiveAgentId(agentId: string | null): void
  setMissingAgentId(agentId: string | null): void
}

const ProductWorkbenchContext =
  createContext<ProductWorkbenchContextValue | null>(null)

const initialResource = <T,>(): WorkbenchResource<T> => ({
  items: [],
  phase: "loading",
  error: null,
})

export function useProductWorkbench(): ProductWorkbenchContextValue {
  const context = useContext(ProductWorkbenchContext)
  if (!context)
    throw new Error("useProductWorkbench must be used inside ProductShell")
  return context
}

function browserStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined
  try {
    return window.localStorage
  } catch {
    return undefined
  }
}

function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error
  return new ApiError(
    "metadata_failed",
    0,
    error instanceof Error ? error.message : "metadata request failed"
  )
}

type HistoryPanelProps = {
  open: boolean
  onClose(): void
  activeAgentId: string | null
  missingAgentId: string | null
  recentAgents: readonly RecentAgent[]
  onRemoveAgent(agentId: string): void
}

function HistoryPanel({
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
            className="fixed inset-0 z-[80] cursor-default bg-background/70 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.18 }}
            onClick={onClose}
          />
          <motion.aside
            ref={panelRef}
            className="stratum-history-panel flex flex-col"
            role="dialog"
            aria-modal="true"
            aria-label={t("productShell.recent")}
            onKeyDown={keepFocusInside}
            initial={{ x: reduceMotion ? 0 : 16, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            exit={{ x: reduceMotion ? 0 : 16, opacity: 0 }}
            transition={{
              duration: reduceMotion ? 0 : 0.24,
              ease: [0.22, 1, 0.36, 1],
            }}
          >
            <div className="flex h-14 items-center justify-between px-4">
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

            <div className="min-h-0 flex-1 overflow-y-auto p-2">
              {recentAgents.length === 0 ? (
                <div className="flex min-h-40 items-center justify-center px-4 text-center text-sm text-muted-foreground">
                  {t("productShell.noRecent")}
                </div>
              ) : (
                <ul className="space-y-1">
                  {recentAgents.map((agent) => {
                    const active = agent.agentId === activeAgentId
                    const missing = agent.agentId === missingAgentId
                    return (
                      <li key={agent.agentId} className="group flex gap-1">
                        <Link
                          to={`/chat?agent=${encodeURIComponent(agent.agentId)}`}
                          onClick={onClose}
                          aria-current={active ? "page" : undefined}
                          className={cn(
                            "flex min-h-12 min-w-0 flex-1 items-center gap-3 rounded-lg px-3 transition-colors duration-200",
                            active
                              ? "bg-secondary text-foreground"
                              : "text-muted-foreground hover:bg-secondary/70 hover:text-foreground",
                            missing && "text-destructive"
                          )}
                        >
                          <IconClock
                            className="size-4 shrink-0"
                            aria-hidden="true"
                          />
                          <span className="min-w-0 flex-1 truncate text-sm">
                            {agent.title}
                          </span>
                          <span className="shrink-0 font-mono text-[0.65rem] text-muted-foreground">
                            {formatRelativeTime(agent.lastOpenedAt, language)}
                          </span>
                        </Link>
                        {missing ? (
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="size-12 text-destructive"
                            onClick={() => onRemoveAgent(agent.agentId)}
                            aria-label={t("chat.removeLocalEntry")}
                          >
                            <IconTrash aria-hidden="true" />
                          </Button>
                        ) : null}
                      </li>
                    )
                  })}
                </ul>
              )}
            </div>
          </motion.aside>
        </>
      ) : null}
    </AnimatePresence>
  )
}

export function ProductShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()
  const [templates, setTemplates] =
    useState<WorkbenchResource<AgentTemplateView>>(initialResource)
  const [models, setModels] =
    useState<WorkbenchResource<ModelDescriptor>>(initialResource)
  const [recentAgents, setRecentAgents] = useState<readonly RecentAgent[]>([])
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null)
  const [missingAgentId, setMissingAgentId] = useState<string | null>(null)
  const [historyOpen, setHistoryOpen] = useState(false)
  const mainRef = useRef<HTMLElement>(null)
  const historyReturnFocusRef = useRef<HTMLElement | null>(null)
  const previousPathRef = useRef(location.pathname)

  const refreshTemplates = useCallback(async () => {
    setTemplates((resource) => ({ ...resource, phase: "loading", error: null }))
    try {
      const items = await createStratumApi({
        baseUrl: STRATUM_API_BASE_URL,
      }).getAgentTemplates()
      setTemplates({
        items,
        phase: items.length === 0 ? "empty" : "loaded",
        error: null,
      })
    } catch (error) {
      setTemplates({ items: [], phase: "error", error: toApiError(error) })
    }
  }, [])

  const refreshModels = useCallback(async () => {
    setModels((resource) => ({ ...resource, phase: "loading", error: null }))
    try {
      const items = await createStratumApi({
        baseUrl: STRATUM_API_BASE_URL,
      }).getModels()
      setModels({
        items,
        phase: items.length === 0 ? "empty" : "loaded",
        error: null,
      })
    } catch (error) {
      setModels({ items: [], phase: "error", error: toApiError(error) })
    }
  }, [])

  useEffect(() => {
    void Promise.allSettled([refreshTemplates(), refreshModels()])
    const storage = browserStorage()
    if (storage) setRecentAgents(loadRecentAgents(storage))
  }, [refreshModels, refreshTemplates])

  const rememberRecentAgent = useCallback((agent: RecentAgent) => {
    const storage = browserStorage()
    if (storage) {
      rememberStoredRecentAgent(storage, agent)
      setRecentAgents(loadRecentAgents(storage))
      return
    }
    setRecentAgents((agents) => [
      agent,
      ...agents.filter((recentAgent) => recentAgent.agentId !== agent.agentId),
    ])
  }, [])

  const removeRecentAgent = useCallback(
    (agentId: string) => {
      const storage = browserStorage()
      if (storage) {
        removeStoredRecentAgent(storage, agentId)
        setRecentAgents(loadRecentAgents(storage))
      } else {
        setRecentAgents((agents) =>
          agents.filter((agent) => agent.agentId !== agentId)
        )
      }
      if (missingAgentId === agentId) setMissingAgentId(null)
      if (activeAgentId === agentId) {
        setActiveAgentId(null)
        navigate("/chat?new=1")
      }
    },
    [activeAgentId, missingAgentId, navigate]
  )

  const openHistory = useCallback(() => {
    historyReturnFocusRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null
    setHistoryOpen(true)
  }, [])

  const closeHistory = useCallback(() => {
    setHistoryOpen(false)
    requestAnimationFrame(() => historyReturnFocusRef.current?.focus())
  }, [])

  useEffect(() => {
    if (previousPathRef.current === location.pathname) return
    previousPathRef.current = location.pathname
    setHistoryOpen(false)
    historyReturnFocusRef.current = null
    const focusFrame = requestAnimationFrame(() => mainRef.current?.focus())
    return () => cancelAnimationFrame(focusFrame)
  }, [location.pathname])

  useEffect(() => {
    if (!historyOpen) return
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return
      closeHistory()
    }
    window.addEventListener("keydown", closeOnEscape)
    return () => window.removeEventListener("keydown", closeOnEscape)
  }, [closeHistory, historyOpen])

  const contextValue = useMemo<ProductWorkbenchContextValue>(
    () => ({
      templates,
      models,
      recentAgents,
      activeAgentId,
      missingAgentId,
      metadataLoading:
        templates.phase === "loading" || models.phase === "loading",
      metadataError: templates.error ?? models.error,
      refreshTemplates,
      refreshModels,
      openHistory,
      rememberRecentAgent,
      removeRecentAgent,
      setActiveAgentId,
      setMissingAgentId,
    }),
    [
      activeAgentId,
      missingAgentId,
      models,
      openHistory,
      recentAgents,
      refreshModels,
      refreshTemplates,
      rememberRecentAgent,
      removeRecentAgent,
      templates,
    ]
  )

  return (
    <ProductWorkbenchContext.Provider value={contextValue}>
      <div className="stratum-app-shell text-foreground">
        <a
          href="#main-content"
          className="fixed top-2 left-2 z-[70] -translate-y-20 rounded-lg bg-primary px-4 py-3 text-sm font-semibold text-primary-foreground focus:translate-y-0"
        >
          {t("productShell.skipToContent")}
        </a>

        <HistoryPanel
          open={historyOpen}
          onClose={closeHistory}
          activeAgentId={activeAgentId}
          missingAgentId={missingAgentId}
          recentAgents={recentAgents}
          onRemoveAgent={removeRecentAgent}
        />

        <main
          ref={mainRef}
          id="main-content"
          className="stratum-main"
          tabIndex={-1}
        >
          {children}
        </main>
      </div>
    </ProductWorkbenchContext.Provider>
  )
}
