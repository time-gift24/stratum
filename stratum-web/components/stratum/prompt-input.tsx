"use client"

import { useState } from "react"
import { ArrowUp, Plus } from "lucide-react"

import { BorderGlow } from "@/components/react-bits/border-glow"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * PromptInput —— Gemini 式药丸提示词输入框。
 * 布局：左侧 + 按钮 / 中间输入 / 右侧模型名（静态展示）与发送（空输入禁用，
 * Enter 提交，IME 组合态除外）。激活态：聚焦时 BorderGlow 全线段点亮——
 * 整圈 mesh 渐变边框 + 全周外发光（port/primary token 配色），失焦 0.75s 淡出。
 * 值默认内部自管（提交后清空），也可传 value/onChange 受控。
 */
export function PromptInput({
  placeholder = "问问 Stratum",
  model,
  value,
  onChange,
  onSubmit,
  className,
}: {
  placeholder?: string
  /** 当前模型名；不传则不显示模型位 */
  model?: string
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
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setFocused(false)
      }}
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
          {model ? (
            <span className="px-2 font-sans text-sm text-muted-foreground">
              {model}
            </span>
          ) : null}
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
