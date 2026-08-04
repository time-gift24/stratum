"use client"

import { memo, useEffect, useState } from "react"
import {
  Check,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Copy,
  Download,
  Pencil,
  RefreshCw,
  RotateCcw,
  TriangleAlert,
} from "lucide-react"
import { Streamdown } from "streamdown"

import type { ConversationMessage } from "@/components/stratum/conversation/types"
import { Reasoning } from "@/components/stratum/conversation/reasoning"
import { ToolGroup } from "@/components/stratum/conversation/tool-group"
import { useSmoothText } from "@/hooks/use-smooth-text"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

import styles from "../styles/prose-medium.module.css"

/**
 * 消息组件（assistant-ui message 底稿的展示层 fork，数据驱动）。
 * UserMessage —— 右侧 muted 气泡，hover 显示编辑入口（可选）。
 * AssistantMessage —— streamdown 渲染正文（streaming 状态带 caret），
 *   底部 = BranchPicker（多版本时）+ ActionBar（复制 / 重生成 / 导出 md）。
 *   非最后一条的 ActionBar 仅 hover 显示（assistant-ui 的 autohide="not-last"）。
 */

function BranchPicker({
  versions,
  active,
  onChange,
  className,
}: {
  versions: number
  active: number
  onChange: (index: number) => void
  className?: string
}) {
  if (versions < 2) return null
  return (
    <div
      data-slot="branch-picker"
      className={cn(
        "inline-flex items-center text-xs text-muted-foreground",
        className
      )}
    >
      <Button
        variant="ghost"
        size="icon-sm"
        className="rounded-full"
        aria-label="上一版本"
        onClick={() => onChange((active - 1 + versions) % versions)}
      >
        <ChevronLeft aria-hidden />
      </Button>
      <span className="font-mono font-medium">
        {active + 1} / {versions}
      </span>
      <Button
        variant="ghost"
        size="icon-sm"
        className="rounded-full"
        aria-label="下一版本"
        onClick={() => onChange((active + 1) % versions)}
      >
        <ChevronRight aria-hidden />
      </Button>
    </div>
  )
}

function ActionBar({
  message,
  body,
  visible,
  onReload,
}: {
  message: ConversationMessage
  /** 当前展示的正文（分支切换后随版本变化） */
  body: string
  visible: boolean
  onReload?: (message: ConversationMessage) => void
}) {
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(() => setCopied(false), 1500)
    return () => clearTimeout(timer)
  }, [copied])

  const copy = () => {
    navigator.clipboard
      ?.writeText(body)
      .then(() => setCopied(true))
      // 权限被拒或非安全上下文时静默降级：不置 copied 态
      .catch(() => {})
  }

  const exportMarkdown = () => {
    const blob = new Blob([body], { type: "text/markdown" })
    const url = URL.createObjectURL(blob)
    const a = document.createElement("a")
    a.href = url
    a.download = `${message.id}.md`
    a.click()
    URL.revokeObjectURL(url)
  }

  return (
    <div
      data-slot="message-action-bar"
      className={cn(
        "flex items-center gap-0.5 text-muted-foreground transition-opacity duration-200",
        visible
          ? "opacity-100"
          : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
      )}
    >
      <Button
        variant="ghost"
        size="icon-sm"
        className="rounded-full"
        aria-label="复制"
        onClick={copy}
      >
        {copied ? (
          <Check aria-hidden className="text-primary" />
        ) : (
          <Copy aria-hidden />
        )}
      </Button>
      {onReload ? (
        <Button
          variant="ghost"
          size="icon-sm"
          className="rounded-full"
          aria-label="重新生成"
          onClick={() => onReload(message)}
        >
          <RefreshCw aria-hidden />
        </Button>
      ) : null}
      <Button
        variant="ghost"
        size="icon-sm"
        className="rounded-full"
        aria-label="导出为 Markdown"
        onClick={exportMarkdown}
      >
        <Download aria-hidden />
      </Button>
    </div>
  )
}

export const UserMessage = memo(function UserMessage({
  message,
  onEdit,
  onRetry,
}: {
  message: ConversationMessage
  onEdit?: (message: ConversationMessage) => void
  /** 发送失败（status="error"）时的重发入口 */
  onRetry?: (message: ConversationMessage) => void
}) {
  const failed = message.status === "error"

  return (
    <div
      data-slot="user-message"
      className="group flex items-start justify-end gap-2 px-2"
    >
      {failed ? (
        <div className="flex items-center gap-1 self-center">
          <p className="flex items-center gap-1 text-xs text-destructive">
            <CircleAlert aria-hidden className="size-3.5" />
            未送达
          </p>
          {onRetry ? (
            <Button
              variant="ghost"
              size="xs"
              className="rounded-full text-destructive hover:text-destructive"
              onClick={() => onRetry(message)}
            >
              <RotateCcw aria-hidden />
              重发
            </Button>
          ) : null}
        </div>
      ) : null}
      {onEdit && !failed ? (
        <Button
          variant="ghost"
          size="icon-sm"
          className="mt-1.5 rounded-full opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100"
          aria-label="编辑"
          onClick={() => onEdit(message)}
        >
          <Pencil aria-hidden />
        </Button>
      ) : null}
      <div
        className={cn(
          "max-w-[85%] rounded-xl px-4 py-2 text-[0.9375rem] leading-relaxed wrap-break-word whitespace-pre-wrap",
          failed
            ? "border border-destructive/40 bg-destructive/10 text-foreground"
            : "bg-muted text-foreground"
        )}
      >
        {message.content}
      </div>
    </div>
  )
})

export const AssistantMessage = memo(function AssistantMessage({
  message,
  isLast,
  onReload,
}: {
  message: ConversationMessage
  isLast?: boolean
  onReload?: (message: ConversationMessage) => void
}) {
  const versions = message.versions
  const [activeVersion, setActiveVersion] = useState(0)

  // 版本数变化（如重新生成追加了新版本）时在渲染期派生跳到最新一版，
  // 不写 effect（React 官方 "derive state during render" 模式）；
  // 无条件钳到最后一版，兼作 versions 缩小时 activeVersion 的越界钳位
  const [prevVersionCount, setPrevVersionCount] = useState(
    versions?.length ?? 0
  )
  if (versions && versions.length !== prevVersionCount) {
    setPrevVersionCount(versions.length)
    setActiveVersion(versions.length - 1)
  }

  const body = versions ? versions[activeVersion] : message.content
  const streaming = message.status === "streaming"
  // streaming 正文过水流平滑（非 streaming 直接透传全文）
  const renderedBody = useSmoothText(body, streaming)

  return (
    <div data-slot="assistant-message" className="group flex flex-col px-2">
      {message.reasoning ? (
        <Reasoning
          text={message.reasoning}
          streaming={streaming}
          defaultView={message.reasoningDefaultView}
        />
      ) : null}
      {message.toolCalls && message.toolCalls.length > 0 ? (
        <ToolGroup calls={message.toolCalls} className="mb-2" />
      ) : null}
      <div
        className={cn(
          styles.proseMedium,
          styles.proseMediumChat,
          "min-h-6 text-foreground"
        )}
      >
        <Streamdown
          mode={streaming ? "streaming" : "static"}
          caret={streaming ? "block" : undefined}
        >
          {renderedBody}
        </Streamdown>
      </div>
      {message.status === "error" ? (
        <div className="mt-2 flex items-center gap-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-foreground">
          <TriangleAlert
            aria-hidden
            className="size-4 shrink-0 text-destructive"
          />
          <p className="min-w-0 flex-1 text-muted-foreground">
            生成中断，以上内容不完整。
          </p>
          {onReload ? (
            <Button
              variant="outline"
              size="xs"
              className="shrink-0 rounded-full border-destructive/40 text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={() => onReload(message)}
            >
              <RotateCcw aria-hidden />
              重试
            </Button>
          ) : null}
        </div>
      ) : null}
      <div className="mt-1.5 flex min-h-7 items-center gap-1">
        {versions ? (
          <BranchPicker
            versions={versions.length}
            active={activeVersion}
            onChange={setActiveVersion}
          />
        ) : null}
        {!streaming ? (
          <ActionBar
            message={message}
            body={body}
            visible={Boolean(isLast)}
            onReload={message.status === "error" ? undefined : onReload}
          />
        ) : null}
      </div>
    </div>
  )
})
