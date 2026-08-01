"use client"

import { useRef, useState } from "react"
import { ArrowUp, Plus } from "lucide-react"

import { BorderGlow } from "@/components/react-bits/border-glow"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * PromptInput —— Gemini 式药丸提示词输入框。
 * 布局：左侧 + 按钮 / 中间输入 / 右侧 trailing 插槽（如模型选择器）与发送（空输入禁用，
 * Enter 提交，IME 组合态除外）。激活态：聚焦时 BorderGlow 全线段点亮——
 * 整圈 mesh 渐变边框 + 全周外发光（port/primary token 配色），失焦 0.75s 淡出。
 * 值默认内部自管（提交后清空），也可传 value/onChange 受控。
 */
export function PromptInput({
  placeholder = "问问 Stratum",
  trailing,
  value,
  onChange,
  onSubmit,
  className,
}: {
  placeholder?: string
  /** 输入框右侧、发送按钮之前的插槽（如模型选择器）；不传则不渲染 */
  trailing?: React.ReactNode
  /** 受控值；不传则内部自管（提交后自动清空） */
  value?: string
  onChange?: (value: string) => void
  onSubmit?: (value: string) => void
  className?: string
}) {
  const [innerValue, setInnerValue] = useState("")
  const controlled = value !== undefined
  const currentValue = controlled ? value : innerValue
  const [focused, setFocused] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const canSend = currentValue.trim().length > 0

  const updateValue = (next: string) => {
    if (controlled) {
      onChange?.(next)
    } else {
      setInnerValue(next)
    }
  }

  const submit = () => {
    if (!canSend) return
    onSubmit?.(currentValue.trim())
    if (!controlled) setInnerValue("")
  }

  return (
    <div
      data-slot="prompt-input"
      className={cn("w-full", className)}
      onFocus={() => setFocused(true)}
      onBlur={() => {
        // popover（如 ModelSelector 内容）portal 到 body，relatedTarget 判断
        // 必然失效；延迟一帧看真实焦点落点——在组件内或任一 popover 内都保持点亮
        requestAnimationFrame(() => {
          const active = document.activeElement
          if (active instanceof HTMLElement) {
            if (rootRef.current?.contains(active)) return
            if (active.closest('[data-slot="popover-content"]')) return
          }
          setFocused(false)
        })
      }}
      ref={rootRef}
    >
      <BorderGlow
        active={focused}
        flat
        borderRadius={999}
        glowRadius={28}
        className="rounded-full"
      >
        <div className="flex items-center gap-1.5 rounded-full p-1.5 shadow-[0_8px_30px] shadow-black/10">
          <Button
            variant="ghost"
            size="icon"
            className="rounded-full"
            aria-label="添加附件"
          >
            <Plus aria-hidden />
          </Button>
          <input
            value={currentValue}
            onChange={(e) => updateValue(e.target.value)}
            onKeyDown={(e) => {
              // 中文/日文输入法下 Enter 是确认候选词，不应提交
              if (e.key === "Enter" && !e.nativeEvent.isComposing) submit()
            }}
            placeholder={placeholder}
            aria-label={placeholder}
            className="min-w-0 flex-1 bg-transparent px-1 font-sans text-sm text-foreground outline-none placeholder:text-muted-foreground"
          />
          {trailing}
          <Button
            size="icon"
            className="rounded-full"
            aria-label="发送"
            disabled={!canSend}
            onClick={submit}
          >
            <ArrowUp aria-hidden />
          </Button>
        </div>
      </BorderGlow>
    </div>
  )
}
