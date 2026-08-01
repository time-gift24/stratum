"use client"

import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { ChevronDown } from "lucide-react"

import { cn } from "@/lib/utils"

/**
 * WorkflowNode —— 画布上的节点卡片（stratum 内部组件），z 轴双层结构：
 * 背板是半透明玻璃（bg-card/50 + backdrop-blur），承载头部与 aurora 光晕；
 * 面板是实心深色卡片（--popover），承载端口与控件。
 * 只消费全局 token，端口颜色通过 NodePort 的语义变体表达。
 * 传入 nodeId 后节点与端口成为可定位锚点（data-node / data-port），
 * 供 WorkflowGraph 测量并绘制连线。
 */

const WorkflowNodeContext = React.createContext<string | null>(null)

function WorkflowNode({
  title,
  status = "idle",
  action,
  floatingAction,
  label,
  nodeId,
  aurora = false,
  className,
  children,
  ...props
}: React.ComponentProps<"section"> & {
  title: string
  status?: VariantProps<typeof statusDotVariants>["tone"]
  action?: React.ReactNode
  /** 悬浮在节点右上方的动作（如 Generate），锚定节点自身，无需外部测量宽度 */
  floatingAction?: React.ReactNode
  /** 悬浮在节点左上方的标签（色点 + 名称） */
  label?: {
    text: string
    tone?: VariantProps<typeof statusDotVariants>["tone"]
  }
  nodeId?: string
  /** 玻璃背板顶部透出极光光晕（Generator 节点） */
  aurora?: boolean
}) {
  return (
    <section
      data-slot="workflow-node"
      data-node={nodeId}
      className={cn(
        "relative w-56 rounded-2xl border border-border bg-card/50 p-2 shadow-[0_8px_30px] shadow-black/35 backdrop-blur-xl",
        className
      )}
      {...props}
    >
      {label ? (
        <WorkflowNodeLabel
          tone={label.tone}
          className="absolute -top-5 left-1 z-10"
        >
          {label.text}
        </WorkflowNodeLabel>
      ) : null}
      {floatingAction ? (
        <div className="absolute -top-7 right-0 z-10">{floatingAction}</div>
      ) : null}
      {aurora ? (
        <div
          aria-hidden
          className="pointer-events-none absolute -top-5 -right-5 h-20 w-2/3 rounded-full opacity-90 blur-xl"
          style={{ background: "var(--node-aurora)" }}
        />
      ) : null}
      <header className="relative flex items-center gap-2 px-2 pt-1 pb-2">
        <span aria-hidden className={cn(statusDotVariants({ tone: status }))} />
        <h3 className="flex-1 truncate font-sans text-xs font-medium tracking-tight">
          {title}
        </h3>
        {action}
        <ChevronDown aria-hidden className="size-3.5 text-muted-foreground" />
      </header>
      <div className="relative rounded-xl border border-border/50 bg-popover px-3.5 py-3">
        <WorkflowNodeContext.Provider value={nodeId ?? null}>
          <div className="flex flex-col gap-2.5">{children}</div>
        </WorkflowNodeContext.Provider>
      </div>
    </section>
  )
}

const statusDotVariants = cva("size-1.5 shrink-0 rounded-full", {
  variants: {
    tone: {
      idle: "bg-muted-foreground",
      model: "bg-port-model",
      positive: "bg-port-positive",
      negative: "bg-port-negative",
      image: "bg-port-image",
    },
  },
  defaultVariants: { tone: "idle" },
})

/**
 * WorkflowNodeLabel —— 悬浮在节点上方的标签（色点 + 名称），不属于卡片本体。
 * 由调用方用 absolute 定位到节点外侧。
 */
function WorkflowNodeLabel({
  tone = "idle",
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  tone?: VariantProps<typeof statusDotVariants>["tone"]
}) {
  return (
    <div
      data-slot="workflow-node-label"
      className={cn(
        "flex items-center gap-1.5 font-sans text-[0.625rem] text-muted-foreground",
        className
      )}
      {...props}
    >
      <span aria-hidden className={cn(statusDotVariants({ tone }))} />
      {children}
    </div>
  )
}

const portDotVariants = cva("size-1.5 shrink-0 rounded-full", {
  variants: {
    tone: {
      model: "bg-port-model",
      positive: "bg-port-positive",
      negative: "bg-port-negative",
      image: "bg-port-image",
    },
  },
})

/**
 * NodePort —— 节点端口行：色点（数据类型语义）+ 标签。
 * align="start" 为输入（左侧），align="end" 为输出（右侧）。
 * 传入 port 且所在 WorkflowNode 有 nodeId 时，色点成为 data-port 锚点。
 */
function NodePort({
  tone,
  port,
  align = "start",
  className,
  children,
  ...props
}: React.ComponentProps<"div"> &
  VariantProps<typeof portDotVariants> & {
    port?: string
    align?: "start" | "end"
  }) {
  const nodeId = React.useContext(WorkflowNodeContext)
  return (
    <div
      data-slot="node-port"
      className={cn(
        "flex items-center gap-1.5 font-sans text-[0.625rem] text-muted-foreground",
        align === "end" && "flex-row-reverse text-right",
        className
      )}
      {...props}
    >
      <span
        aria-hidden
        data-port={nodeId && port ? `${nodeId}:${port}` : undefined}
        className={cn(portDotVariants({ tone }))}
      />
      {children}
    </div>
  )
}

export { WorkflowNode, WorkflowNodeLabel, NodePort }
