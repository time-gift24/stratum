"use client"

import { useState, type ReactNode } from "react"

import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

/**
 * 编辑面板共享控件：失焦/回车提交的文本输入（本地草稿态，非法值不提交）。
 * name 类字段经 validate 先行校验。
 */

export function FieldRow({
  label,
  htmlFor,
  error,
  children,
}: {
  label: string
  htmlFor?: string
  error?: string | null
  children: ReactNode
}) {
  return (
    <div className="flex flex-col gap-1">
      <label
        htmlFor={htmlFor}
        className="text-[0.6875rem] font-medium text-muted-foreground"
      >
        {label}
      </label>
      {children}
      {error !== undefined && error !== null && error !== "" && (
        <p role="alert" className="text-[0.6875rem] text-destructive">
          {error}
        </p>
      )}
    </div>
  )
}

export function CommitInput({
  value,
  onCommit,
  validate,
  placeholder,
  mono,
  ariaLabel,
  autoFocus,
  className,
}: {
  value: string
  onCommit(next: string): void
  validate?(next: string): string | null
  placeholder?: string
  mono?: boolean
  ariaLabel?: string
  /** 挂载即聚焦（如画布节点行内改名、新建后立即编辑） */
  autoFocus?: boolean
  /** 覆盖 Input 外观（如表格单元格的 ghost 样式） */
  className?: string
}) {
  const [draft, setDraft] = useState(value)
  const [error, setError] = useState<string | null>(null)

  // 外部值变化（切换选中项 / 远端调和）时重置本地草稿
  const [prevValue, setPrevValue] = useState(value)
  if (value !== prevValue) {
    setPrevValue(value)
    setDraft(value)
    setError(null)
  }

  const commit = () => {
    if (draft === value) {
      setError(null)
      return
    }
    const message = validate?.(draft) ?? null
    if (message !== null) {
      setError(message)
      return
    }
    setError(null)
    onCommit(draft)
  }

  return (
    <>
      <Input
        aria-label={ariaLabel}
        aria-invalid={error !== null}
        value={draft}
        placeholder={placeholder}
        autoFocus={autoFocus}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur()
        }}
        className={cn(mono && "font-mono", className)}
      />
      {error !== null && (
        <p role="alert" className="text-[0.6875rem] text-destructive">
          {error}
        </p>
      )}
    </>
  )
}

export function CommitTextarea({
  value,
  onCommit,
  placeholder,
  ariaLabel,
}: {
  value: string
  onCommit(next: string): void
  placeholder?: string
  ariaLabel?: string
}) {
  const [draft, setDraft] = useState(value)

  const [prevValue, setPrevValue] = useState(value)
  if (value !== prevValue) {
    setPrevValue(value)
    setDraft(value)
  }

  return (
    <Textarea
      aria-label={ariaLabel}
      value={draft}
      placeholder={placeholder}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft !== value) onCommit(draft)
      }}
    />
  )
}
