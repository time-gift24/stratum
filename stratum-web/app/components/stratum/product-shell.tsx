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
import { useGSAP } from "@gsap/react"
import {
  IconAlertCircle,
  IconClock,
  IconHistory,
  IconHome,
  IconLoader2,
  IconMenu2,
  IconMessageCircle,
  IconPlus,
  IconTrash,
  IconX,
} from "@tabler/icons-react"
import gsap from "gsap"
import { AnimatePresence, motion, useReducedMotion } from "motion/react"
import { Link, useLocation, useNavigate } from "react-router"
import { useTranslation } from "react-i18next"

import { LanguageToggle } from "~/components/stratum/language-toggle"
import { StratumMark } from "~/components/stratum/stratum-mark"
import { Button, buttonVariants } from "~/components/ui/button"
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

gsap.registerPlugin(useGSAP)

type ResourcePhase = "loading" | "ready" | "empty" | "error"

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

type RuntimeStatus = {
  kind: "loading" | "ready" | "incomplete" | "error"
  label: string
}

function RuntimeStatusBadge({ status }: { status: RuntimeStatus }) {
  return (
    <div
      className={cn(
        "flex min-h-9 items-center gap-2 rounded-[0.65rem] border border-border bg-secondary/70 px-3 font-mono text-[0.7rem] text-muted-foreground",
        status.kind === "ready" && "text-primary",
        status.kind === "error" && "text-destructive"
      )}
      role="status"
    >
      {status.kind === "loading" ? (
        <IconLoader2
          className="size-3.5 animate-spin motion-reduce:animate-none"
          aria-hidden="true"
        />
      ) : status.kind === "error" ? (
        <IconAlertCircle className="size-3.5" aria-hidden="true" />
      ) : (
        <span
          className={cn(
            "size-1.5 rounded-full bg-muted-foreground",
            status.kind === "ready" &&
              "bg-primary [box-shadow:0_0_10px_color-mix(in_srgb,var(--primary)_55%,transparent)]"
          )}
          aria-hidden="true"
        />
      )}
      <span className="hidden max-w-48 truncate sm:inline">{status.label}</span>
    </div>
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

  return (
    <AnimatePresence initial={false}>
      {open ? (
        <>
          <motion.button
            type="button"
            aria-label={t("chat.history.close")}
            className="fixed inset-0 z-40 cursor-default bg-black/45"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.18 }}
            onClick={onClose}
          />
          <motion.aside
            className="stratum-history-panel flex flex-col"
            role="dialog"
            aria-modal="true"
            aria-label={t("productShell.recent")}
            initial={{ x: reduceMotion ? 0 : -18, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            exit={{ x: reduceMotion ? 0 : -18, opacity: 0 }}
            transition={{
              duration: reduceMotion ? 0 : 0.24,
              ease: [0.22, 1, 0.36, 1],
            }}
          >
            <div className="flex h-14 items-center justify-between border-b border-border px-4">
              <div className="flex items-center gap-2.5">
                <IconHistory
                  className="size-4 text-primary"
                  aria-hidden="true"
                />
                <h2 className="font-heading text-sm font-medium text-foreground">
                  {t("productShell.recent")}
                </h2>
              </div>
              <Button
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

            <div className="border-b border-border p-3">
              <Link
                to="/chat?new=1"
                onClick={onClose}
                className={cn(
                  buttonVariants({ variant: "default", size: "lg" }),
                  "w-full justify-between"
                )}
              >
                {t("productShell.newConversation")}
                <IconPlus aria-hidden="true" />
              </Link>
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
                            "flex min-h-12 min-w-0 flex-1 items-center gap-3 rounded-[0.65rem] border border-transparent px-3 transition-colors duration-200",
                            active
                              ? "border-border bg-secondary text-foreground"
                              : "text-muted-foreground hover:border-border hover:bg-secondary/70 hover:text-foreground",
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
  const reduceMotion = useReducedMotion()
  const activeSection = location.pathname.startsWith("/chat")
    ? "chat"
    : "overview"
  const [templates, setTemplates] =
    useState<WorkbenchResource<AgentTemplateView>>(initialResource)
  const [models, setModels] =
    useState<WorkbenchResource<ModelDescriptor>>(initialResource)
  const [recentAgents, setRecentAgents] = useState<readonly RecentAgent[]>([])
  const [activeAgentId, setActiveAgentId] = useState<string | null>(null)
  const [missingAgentId, setMissingAgentId] = useState<string | null>(null)
  const [mobileNavigationOpen, setMobileNavigationOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const shellRef = useRef<HTMLDivElement>(null)
  const mainRef = useRef<HTMLElement>(null)
  const previousPathRef = useRef(location.pathname)

  const refreshTemplates = useCallback(async () => {
    setTemplates((resource) => ({ ...resource, phase: "loading", error: null }))
    try {
      const items = await createStratumApi({
        baseUrl: STRATUM_API_BASE_URL,
      }).getAgentTemplates()
      setTemplates({
        items,
        phase: items.length === 0 ? "empty" : "ready",
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
        phase: items.length === 0 ? "empty" : "ready",
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

  useEffect(() => {
    if (previousPathRef.current === location.pathname) return
    previousPathRef.current = location.pathname
    setMobileNavigationOpen(false)
    setHistoryOpen(false)
    const focusFrame = requestAnimationFrame(() => mainRef.current?.focus())
    return () => cancelAnimationFrame(focusFrame)
  }, [location.pathname])

  useEffect(() => {
    if (!mobileNavigationOpen) return
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = "hidden"
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMobileNavigationOpen(false)
    }
    window.addEventListener("keydown", closeOnEscape)
    return () => {
      window.removeEventListener("keydown", closeOnEscape)
      document.body.style.overflow = previousOverflow
    }
  }, [mobileNavigationOpen])

  useGSAP(
    () => {
      if (reduceMotion) return
      gsap.fromTo(
        "[data-shell-animate]",
        { y: -10, opacity: 0 },
        {
          y: 0,
          opacity: 1,
          duration: 0.55,
          stagger: 0.045,
          ease: "power3.out",
          clearProps: "transform,opacity",
        }
      )
    },
    { scope: shellRef, dependencies: [reduceMotion] }
  )

  const status = useMemo<RuntimeStatus>(() => {
    if (templates.phase === "loading" || models.phase === "loading")
      return { kind: "loading", label: t("productShell.status.loading") }
    if (templates.phase === "error" || models.phase === "error")
      return { kind: "error", label: t("productShell.status.error") }
    if (templates.phase === "empty" || models.phase === "empty")
      return { kind: "incomplete", label: t("productShell.status.incomplete") }
    return { kind: "ready", label: t("productShell.status.ready") }
  }, [models.phase, t, templates.phase])

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
      rememberRecentAgent,
      removeRecentAgent,
      setActiveAgentId,
      setMissingAgentId,
    }),
    [
      activeAgentId,
      missingAgentId,
      models,
      recentAgents,
      refreshModels,
      refreshTemplates,
      rememberRecentAgent,
      removeRecentAgent,
      templates,
    ]
  )

  const navigation = [
    {
      id: "overview" as const,
      label: t("nav.overview"),
      to: "/",
      icon: IconHome,
    },
    {
      id: "chat" as const,
      label: t("nav.chat"),
      to: "/chat",
      icon: IconMessageCircle,
    },
  ]

  return (
    <ProductWorkbenchContext.Provider value={contextValue}>
      <div ref={shellRef} className="stratum-app-shell text-foreground">
        <a
          href="#main-content"
          className="fixed top-2 left-2 z-[70] -translate-y-20 rounded-md bg-primary px-4 py-3 text-sm font-semibold text-primary-foreground focus:translate-y-0"
        >
          {t("productShell.skipToContent")}
        </a>

        <header
          className="stratum-shell-topbar flex items-center px-2.5"
          data-shell-animate
        >
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="mr-1 size-10 lg:hidden"
            onClick={() => setMobileNavigationOpen(true)}
            aria-label={t("productShell.openNavigation")}
            aria-expanded={mobileNavigationOpen}
          >
            <IconMenu2 aria-hidden="true" />
          </Button>

          <Link
            to="/"
            className="flex h-10 items-center gap-2.5 rounded-[0.65rem] px-2 text-foreground"
            aria-label={`运筹 ${t("brand.home")}`}
          >
            <StratumMark variant="compact" className="size-7" />
            <span className="hidden font-heading text-[0.95rem] font-medium tracking-[-0.02em] sm:inline">
              运筹
            </span>
          </Link>

          <div className="mx-2 hidden h-5 w-px bg-border md:block" />

          <nav
            aria-label={t("productShell.navigation")}
            className="hidden items-center gap-1 md:flex"
          >
            {navigation.map((item) => (
              <Link
                key={item.id}
                to={item.to}
                aria-current={activeSection === item.id ? "page" : undefined}
                className={cn(
                  "flex h-9 items-center rounded-[0.6rem] border px-3 text-xs font-medium transition-colors duration-200",
                  activeSection === item.id
                    ? "border-border bg-secondary text-foreground"
                    : "border-transparent text-muted-foreground hover:border-border hover:bg-secondary/70 hover:text-foreground"
                )}
              >
                {item.label}
              </Link>
            ))}
          </nav>

          <div className="pointer-events-none absolute left-1/2 hidden -translate-x-1/2 items-center gap-2 xl:flex">
            <span className="font-mono text-[0.68rem] text-muted-foreground">
              stratum
            </span>
            <span className="size-1 rounded-full bg-border" />
            <span className="text-xs font-medium text-foreground">
              {activeSection === "chat" ? t("nav.chat") : t("nav.overview")}
            </span>
          </div>

          <div className="ml-auto flex items-center gap-1.5">
            <RuntimeStatusBadge status={status} />
            <Link
              to="/chat?new=1"
              className={cn(
                buttonVariants({ size: "default" }),
                "hidden h-9 px-3 sm:inline-flex"
              )}
            >
              <IconPlus data-icon="inline-start" aria-hidden="true" />
              {t("productShell.newConversation")}
            </Link>
            <LanguageToggle compact />
          </div>
        </header>

        <aside
          className="stratum-tool-rail hidden flex-col items-center py-2 lg:flex"
          data-shell-animate
          aria-label={t("productShell.navigation")}
        >
          <nav className="flex w-full flex-col items-center gap-1 px-1.5">
            {navigation.map((item) => {
              const Icon = item.icon
              return (
                <Link
                  key={item.id}
                  to={item.to}
                  title={item.label}
                  aria-label={item.label}
                  aria-current={activeSection === item.id ? "page" : undefined}
                  className="stratum-shell-control relative w-full"
                >
                  <Icon className="size-[1.1rem]" aria-hidden="true" />
                </Link>
              )
            })}
          </nav>

          <div className="my-2 h-px w-7 bg-border" />

          <Link
            to="/chat?new=1"
            title={t("productShell.newConversation")}
            aria-label={t("productShell.newConversation")}
            className="stratum-shell-control relative mx-1.5 w-[calc(100%-0.75rem)] text-primary"
          >
            <IconPlus className="size-[1.1rem]" aria-hidden="true" />
          </Link>
          <button
            type="button"
            title={t("productShell.recent")}
            aria-label={t("productShell.recent")}
            aria-expanded={historyOpen}
            className="stratum-shell-control relative mx-1.5 w-[calc(100%-0.75rem)]"
            onClick={() => setHistoryOpen((open) => !open)}
          >
            <IconHistory className="size-[1.1rem]" aria-hidden="true" />
          </button>

          <div className="mt-auto flex flex-col items-center gap-2 pb-1">
            <span
              className={cn(
                "size-2 rounded-full bg-muted-foreground",
                status.kind === "ready" &&
                  "bg-primary [box-shadow:0_0_12px_color-mix(in_srgb,var(--primary)_52%,transparent)]",
                status.kind === "error" && "bg-destructive"
              )}
              title={status.label}
              aria-hidden="true"
            />
          </div>
        </aside>

        <AnimatePresence initial={false}>
          {mobileNavigationOpen ? (
            <motion.div
              className="fixed inset-0 z-[60] bg-background/98 p-3 lg:hidden"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: reduceMotion ? 0 : 0.2 }}
            >
              <div className="flex h-full flex-col rounded-xl border border-border bg-sidebar p-3">
                <div className="flex h-12 items-center justify-between">
                  <div className="flex items-center gap-3">
                    <StratumMark variant="compact" className="size-8" />
                    <span className="font-heading text-base font-medium">
                      运筹
                    </span>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-10"
                    onClick={() => setMobileNavigationOpen(false)}
                    aria-label={t("productShell.closeNavigation")}
                  >
                    <IconX aria-hidden="true" />
                  </Button>
                </div>
                <nav className="mt-6 space-y-2">
                  {navigation.map((item) => {
                    const Icon = item.icon
                    return (
                      <Link
                        key={item.id}
                        to={item.to}
                        className={cn(
                          "flex min-h-14 items-center gap-3 rounded-lg border px-4 text-base",
                          activeSection === item.id
                            ? "border-border bg-secondary text-foreground"
                            : "border-transparent text-muted-foreground"
                        )}
                      >
                        <Icon className="size-5" aria-hidden="true" />
                        {item.label}
                      </Link>
                    )
                  })}
                </nav>
                <div className="mt-3">
                  <button
                    type="button"
                    className="flex min-h-14 w-full items-center gap-3 rounded-lg border border-transparent px-4 text-base text-muted-foreground"
                    onClick={() => {
                      setMobileNavigationOpen(false)
                      setHistoryOpen(true)
                    }}
                  >
                    <IconHistory className="size-5" aria-hidden="true" />
                    {t("productShell.recent")}
                  </button>
                </div>
                <div className="mt-auto border-t border-border pt-3">
                  <Link
                    to="/chat?new=1"
                    className={cn(
                      buttonVariants({ size: "lg" }),
                      "w-full justify-between"
                    )}
                  >
                    {t("productShell.newConversation")}
                    <IconPlus aria-hidden="true" />
                  </Link>
                </div>
              </div>
            </motion.div>
          ) : null}
        </AnimatePresence>

        <HistoryPanel
          open={historyOpen}
          onClose={() => setHistoryOpen(false)}
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
