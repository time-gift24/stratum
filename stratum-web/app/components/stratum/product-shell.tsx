"use client"

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { useLocation, useNavigate } from "react-router"
import { useTranslation } from "react-i18next"

import { HistoryPanel } from "~/components/stratum/history-panel"
import {
  VerticalNavigation,
  type VerticalNavigationItem,
} from "~/components/stratum/vertical-navigation"
import { CHAT_NAVIGATION_DEFINITIONS } from "~/config/navigation"
import type { AgentTemplateView, ModelDescriptor } from "~/lib/model-config"
import {
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
  const navigationItems = useMemo<readonly VerticalNavigationItem[]>(
    () =>
      CHAT_NAVIGATION_DEFINITIONS.map((item) => ({
        id: item.id,
        icon: item.icon,
        label: t(item.labelKey),
        href: "href" in item ? item.href : undefined,
        onSelect:
          "action" in item && item.action === "open-history"
            ? openHistory
            : undefined,
        tone: item.tone,
      })),
    [openHistory, t]
  )

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
      <div className="min-h-dvh bg-background text-foreground">
        <a
          href="#main-content"
          className="fixed top-2 left-2 z-(--z-navigation) -translate-y-20 rounded-lg bg-primary px-4 py-3 text-sm font-semibold text-primary-foreground focus:translate-y-0"
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

        {location.pathname === "/chat" ? (
          <VerticalNavigation
            activeId={
              activeAgentId === null
                ? "new-conversation"
                : "active-conversation"
            }
            ariaLabel={t("globalNavigation.chat")}
            items={navigationItems}
          />
        ) : null}

        <main
          ref={mainRef}
          id="main-content"
          className="min-h-dvh pt-(--global-nav-offset)"
          tabIndex={-1}
        >
          {children}
        </main>
      </div>
    </ProductWorkbenchContext.Provider>
  )
}
