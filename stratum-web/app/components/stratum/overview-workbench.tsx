"use client"

import {
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react"
import { useGSAP } from "@gsap/react"
import {
  IconArrowRight,
  IconBolt,
  IconClock,
  IconCpu,
  IconFocus2,
  IconMessageCircle,
  IconMinus,
  IconPlus,
  IconRefresh,
  IconRobot,
} from "@tabler/icons-react"
import gsap from "gsap"
import { useReducedMotion } from "motion/react"
import { Link } from "react-router"
import { useTranslation } from "react-i18next"

import { useProductWorkbench } from "~/components/stratum/product-shell"
import { Button, buttonVariants } from "~/components/ui/button"
import { modelDisplayName } from "~/lib/model-config"
import { formatRelativeTime } from "~/lib/recent-agents"
import { cn } from "~/lib/utils"

gsap.registerPlugin(useGSAP)

type NodeId = "input" | "agent" | "model" | "output"
type NodeTone = "positive" | "agent" | "model" | "output"

type WorkspaceNodeProps = {
  id: NodeId
  title: string
  typeLabel: string
  tone: NodeTone
  icon: typeof IconRobot
  selected: boolean
  positionClass: string
  onSelect(id: NodeId): void
  children: ReactNode
}

function WorkspaceNode({
  id,
  title,
  typeLabel,
  tone,
  icon: Icon,
  selected,
  positionClass,
  onSelect,
  children,
}: WorkspaceNodeProps) {
  const selectOnKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return
    event.preventDefault()
    onSelect(id)
  }

  return (
    <article
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-tone={tone}
      data-node-id={id}
      onClick={() => onSelect(id)}
      onKeyDown={selectOnKeyDown}
      className={cn("workspace-node", positionClass)}
    >
      <header className="flex items-center gap-2.5 border-b border-border px-4 py-3">
        <span className="workspace-node-port" aria-hidden="true" />
        <Icon className="size-4 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-foreground">
          {title}
        </span>
        <span className="font-mono text-[0.62rem] text-muted-foreground">
          {typeLabel}
        </span>
      </header>
      <div className="px-4 py-3.5">{children}</div>
    </article>
  )
}

function LoadingLines() {
  return (
    <div className="space-y-2" aria-hidden="true">
      <div className="h-3 w-3/4 animate-pulse rounded-sm bg-muted motion-reduce:animate-none" />
      <div className="h-3 w-1/2 animate-pulse rounded-sm bg-muted motion-reduce:animate-none" />
    </div>
  )
}

export function OverviewWorkbench() {
  const { t, i18n } = useTranslation()
  const reduceMotion = useReducedMotion()
  const workspaceRef = useRef<HTMLDivElement>(null)
  const [selectedNode, setSelectedNode] = useState<NodeId>("agent")
  const [zoom, setZoom] = useState(1)
  const { templates, models, recentAgents, refreshTemplates, refreshModels } =
    useProductWorkbench()
  const template = templates.items[0]
  const model = models.items[0]
  const recent = recentAgents[0]
  const templateModel = template
    ? modelDisplayName(template.model_config.model)
    : null
  const modelName = model ? modelDisplayName(model.model) : null
  const language = i18n.resolvedLanguage ?? "en"

  useGSAP(
    () => {
      if (reduceMotion) return
      const timeline = gsap.timeline({ defaults: { ease: "power3.out" } })
      timeline
        .fromTo(
          ".workspace-node",
          { y: 18, scale: 0.975, opacity: 0, filter: "blur(8px)" },
          {
            y: 0,
            scale: 1,
            opacity: 1,
            filter: "blur(0px)",
            duration: 0.62,
            stagger: 0.085,
            clearProps: "transform,opacity,filter",
          }
        )
        .fromTo(
          ".workspace-wire",
          { strokeDasharray: 1, strokeDashoffset: 1, opacity: 0 },
          {
            strokeDashoffset: 0,
            opacity: 1,
            duration: 0.75,
            stagger: 0.08,
          },
          "-=0.35"
        )
        .fromTo(
          ".workspace-inspector",
          { x: 16, opacity: 0 },
          { x: 0, opacity: 1, duration: 0.45, clearProps: "transform,opacity" },
          "-=0.48"
        )
    },
    { scope: workspaceRef, dependencies: [reduceMotion] }
  )

  const selected = useMemo(() => {
    if (selectedNode === "input") {
      return {
        icon: IconMessageCircle,
        tone: "positive" as const,
        type: t("overview.nodeTypes.input"),
        title: t("overview.inputNode.title"),
        status: t("overview.status.ready"),
        rows: [
          [t("overview.inspector.action"), t("overview.inputNode.action")],
          [t("overview.inspector.route"), "/chat"],
        ],
        action: (
          <Link
            to="/chat?new=1"
            className={cn(buttonVariants({ size: "lg" }), "w-full")}
          >
            {t("overview.startConversation")}
            <IconArrowRight data-icon="inline-end" aria-hidden="true" />
          </Link>
        ),
      }
    }

    if (selectedNode === "agent") {
      return {
        icon: IconRobot,
        tone: "agent" as const,
        type: t("overview.nodeTypes.agent"),
        title: template?.agent_name ?? t("overview.noTemplates"),
        status: t(`overview.status.${templates.phase}`),
        rows: [
          [t("overview.inspector.templates"), String(templates.items.length)],
          [
            t("overview.inspector.model"),
            templateModel
              ? `${templateModel.provider ?? ""} ${templateModel.model}`.trim()
              : t("overview.noneAvailable"),
          ],
        ],
        action:
          templates.phase === "error" ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="w-full"
              onClick={() => void refreshTemplates()}
            >
              <IconRefresh data-icon="inline-start" aria-hidden="true" />
              {t("overview.retry")}
            </Button>
          ) : template ? (
            <Link
              to={`/chat?template=${encodeURIComponent(template.agent_name)}`}
              className={cn(buttonVariants({ size: "lg" }), "w-full")}
            >
              {t("overview.useAgent")}
              <IconArrowRight data-icon="inline-end" aria-hidden="true" />
            </Link>
          ) : null,
      }
    }

    if (selectedNode === "model") {
      return {
        icon: IconCpu,
        tone: "model" as const,
        type: t("overview.nodeTypes.model"),
        title: modelName?.model ?? t("overview.noneAvailable"),
        status: t(`overview.status.${models.phase}`),
        rows: [
          [t("overview.inspector.models"), String(models.items.length)],
          [
            t("overview.inspector.provider"),
            modelName?.provider ?? t("overview.noneAvailable"),
          ],
        ],
        action:
          models.phase === "error" ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="w-full"
              onClick={() => void refreshModels()}
            >
              <IconRefresh data-icon="inline-start" aria-hidden="true" />
              {t("overview.retry")}
            </Button>
          ) : null,
      }
    }

    return {
      icon: IconClock,
      tone: "output" as const,
      type: t("overview.nodeTypes.output"),
      title: recent?.title ?? t("overview.noRecent"),
      status: recent ? t("overview.status.ready") : t("overview.status.empty"),
      rows: [
        [t("overview.inspector.conversations"), String(recentAgents.length)],
        [
          t("overview.inspector.lastOpened"),
          recent
            ? formatRelativeTime(recent.lastOpenedAt, language)
            : t("overview.noneAvailable"),
        ],
      ],
      action: recent ? (
        <Link
          to={`/chat?agent=${encodeURIComponent(recent.agentId)}`}
          className={cn(buttonVariants({ size: "lg" }), "w-full")}
        >
          {t("overview.continueConversation")}
          <IconArrowRight data-icon="inline-end" aria-hidden="true" />
        </Link>
      ) : null,
    }
  }, [
    language,
    modelName,
    models.items.length,
    models.phase,
    recent,
    recentAgents.length,
    refreshModels,
    refreshTemplates,
    selectedNode,
    t,
    template,
    templateModel,
    templates.items.length,
    templates.phase,
  ])

  const SelectedIcon = selected.icon

  return (
    <div ref={workspaceRef} className="stratum-workspace">
      <section className="workspace-stage" aria-label={t("overview.workspace")}>
        <div className="absolute top-5 left-5 z-10 lg:top-6 lg:left-7">
          <div className="flex items-center gap-2.5">
            <IconBolt className="size-4 text-primary" aria-hidden="true" />
            <h1 className="font-heading text-base font-medium tracking-[-0.02em] text-foreground">
              {t("overview.workspace")}
            </h1>
          </div>
        </div>

        <div
          className="workspace-canvas-content origin-center transition-transform duration-200 ease-out motion-reduce:transition-none"
          style={{ "--workspace-zoom": zoom } as CSSProperties}
        >
          <svg
            className="workspace-wires"
            viewBox="0 0 1000 700"
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            <path
              pathLength="1"
              className="workspace-wire is-active"
              d="M 215 360 C 300 360, 270 215, 385 215"
            />
            <path
              pathLength="1"
              className="workspace-wire"
              d="M 215 360 C 310 360, 285 505, 385 505"
            />
            <path
              pathLength="1"
              className="workspace-wire is-active"
              d="M 610 215 C 700 215, 675 350, 785 350"
            />
            <path
              pathLength="1"
              className="workspace-wire"
              d="M 610 505 C 710 505, 675 350, 785 350"
            />
          </svg>

          <WorkspaceNode
            id="input"
            title={t("overview.inputNode.title")}
            typeLabel={t("overview.nodeTypes.input")}
            tone="positive"
            icon={IconMessageCircle}
            selected={selectedNode === "input"}
            positionClass="top-[43%] left-[6%]"
            onSelect={setSelectedNode}
          >
            <p className="text-sm leading-6 text-foreground">
              {t("overview.inputNode.value")}
            </p>
            <div className="mt-3 flex items-center justify-between border-t border-border pt-3 font-mono text-[0.65rem] text-muted-foreground">
              <span>{t("overview.inputNode.action")}</span>
              <IconArrowRight
                className="size-3.5 text-primary"
                aria-hidden="true"
              />
            </div>
          </WorkspaceNode>

          <WorkspaceNode
            id="agent"
            title={template?.agent_name ?? t("overview.noTemplates")}
            typeLabel={t("overview.nodeTypes.agent")}
            tone="agent"
            icon={IconRobot}
            selected={selectedNode === "agent"}
            positionClass="top-[20%] left-[35%]"
            onSelect={setSelectedNode}
          >
            {templates.phase === "loading" ? (
              <LoadingLines />
            ) : (
              <>
                <p className="truncate text-sm text-foreground">
                  {templateModel?.model ?? t("overview.noneAvailable")}
                </p>
                <div className="mt-3 flex items-center justify-between border-t border-border pt-3 font-mono text-[0.65rem] text-muted-foreground">
                  <span>{t("overview.agentTemplates")}</span>
                  <span className="text-chart-4 tabular-nums">
                    {templates.items.length}
                  </span>
                </div>
              </>
            )}
          </WorkspaceNode>

          <WorkspaceNode
            id="model"
            title={modelName?.model ?? t("overview.noneAvailable")}
            typeLabel={t("overview.nodeTypes.model")}
            tone="model"
            icon={IconCpu}
            selected={selectedNode === "model"}
            positionClass="top-[58%] left-[35%]"
            onSelect={setSelectedNode}
          >
            {models.phase === "loading" ? (
              <LoadingLines />
            ) : (
              <>
                <p className="truncate text-sm text-foreground">
                  {modelName?.provider ?? t("overview.noneAvailable")}
                </p>
                <div className="mt-3 flex items-center justify-between border-t border-border pt-3 font-mono text-[0.65rem] text-muted-foreground">
                  <span>{t("overview.models")}</span>
                  <span className="text-chart-2 tabular-nums">
                    {models.items.length}
                  </span>
                </div>
              </>
            )}
          </WorkspaceNode>

          <WorkspaceNode
            id="output"
            title={recent?.title ?? t("overview.noRecent")}
            typeLabel={t("overview.nodeTypes.output")}
            tone="output"
            icon={IconClock}
            selected={selectedNode === "output"}
            positionClass="top-[42%] right-[6%]"
            onSelect={setSelectedNode}
          >
            <p className="truncate text-sm text-foreground">
              {recent
                ? formatRelativeTime(recent.lastOpenedAt, language)
                : t("overview.outputNode.empty")}
            </p>
            <div className="mt-3 flex items-center justify-between border-t border-border pt-3 font-mono text-[0.65rem] text-muted-foreground">
              <span>{t("overview.recentConversations")}</span>
              <span className="text-chart-3 tabular-nums">
                {recentAgents.length}
              </span>
            </div>
          </WorkspaceNode>
        </div>

        <div
          className="workspace-floating-tools"
          aria-label={t("overview.zoom")}
        >
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-9"
            onClick={() => setZoom((value) => Math.max(0.9, value - 0.1))}
            aria-label={t("overview.zoomOut")}
          >
            <IconMinus aria-hidden="true" />
          </Button>
          <span className="grid min-w-10 place-items-center font-mono text-[0.65rem] text-muted-foreground">
            {Math.round(zoom * 100)}%
          </span>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-9"
            onClick={() => setZoom((value) => Math.min(1.1, value + 0.1))}
            aria-label={t("overview.zoomIn")}
          >
            <IconPlus aria-hidden="true" />
          </Button>
          <div className="mx-1 h-5 w-px self-center bg-border" />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-9"
            onClick={() => setZoom(1)}
            aria-label={t("overview.resetZoom")}
          >
            <IconFocus2 aria-hidden="true" />
          </Button>
        </div>
      </section>

      <aside
        className="workspace-inspector"
        aria-label={t("overview.inspector.title")}
      >
        <div className="flex h-14 items-center gap-3 border-b border-border px-4">
          <span
            className="workspace-node-port"
            data-tone={selected.tone}
            aria-hidden="true"
          />
          <SelectedIcon
            className="size-4 text-muted-foreground"
            aria-hidden="true"
          />
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-sm font-medium text-foreground">
              {selected.title}
            </h2>
          </div>
          <span className="font-mono text-[0.62rem] text-muted-foreground">
            {selected.type}
          </span>
        </div>

        <div className="px-4 py-4">
          <div className="flex items-center justify-between gap-4 rounded-[0.65rem] bg-secondary px-3 py-2.5">
            <span className="text-xs text-muted-foreground">
              {t("overview.inspector.status")}
            </span>
            <span className="flex items-center gap-2 font-mono text-[0.68rem] text-foreground">
              <span
                className="size-1.5 rounded-full bg-primary"
                aria-hidden="true"
              />
              {selected.status}
            </span>
          </div>
        </div>

        <div>
          {selected.rows.map(([label, value]) => (
            <div key={label} className="workspace-inspector-row">
              <span className="text-xs text-muted-foreground">{label}</span>
              <span className="min-w-0 truncate text-right font-mono text-[0.7rem] text-foreground">
                {value}
              </span>
            </div>
          ))}
        </div>

        {selected.action ? (
          <div className="absolute right-0 bottom-0 left-0 border-t border-border p-4">
            {selected.action}
          </div>
        ) : null}
      </aside>
    </div>
  )
}
