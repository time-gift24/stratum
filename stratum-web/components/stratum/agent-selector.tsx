"use client"

import { useState } from "react"
import { Check, Plus } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import type { AgentTemplateView } from "@/lib/stratum/model-config"

/**
 * AgentSelector —— 新会话的 Agent template 选择器（composer 左侧 + 按钮触发，
 * popover 向上展开）。与 ModelSelector 同语言：rounded-xl popover、选中打勾。
 * template 数量少，不提供搜索；已选 runtime 的会话由调用方决定不渲染本组件。
 */
export function AgentSelector({
  templates,
  selectedTemplate,
  onSelectTemplate,
}: {
  templates: readonly AgentTemplateView[]
  /** 当前生效的 template；null 表示尚未派生默认项 */
  selectedTemplate: AgentTemplateView | null
  onSelectTemplate: (template: AgentTemplateView) => void
}) {
  const [open, setOpen] = useState(false)

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            className="size-11 rounded-full sm:size-7"
            aria-label="选择 Agent"
          />
        }
      >
        <Plus aria-hidden />
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        sideOffset={8}
        className="w-64 gap-0 overflow-hidden rounded-xl bg-popover p-1 shadow-lg"
      >
        <p className="px-3 pt-2 pb-1 text-xs text-muted-foreground">
          选择 Agent
        </p>
        <ul role="listbox" aria-label="Agent template">
          {templates.map((template) => {
            const selected =
              template.agent_name === selectedTemplate?.agent_name &&
              template.version === selectedTemplate?.version
            return (
              <li key={`${template.agent_name}:${template.version}`}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  onClick={() => {
                    onSelectTemplate(template)
                    setOpen(false)
                  }}
                  className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm outline-none transition-colors hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  <span className="min-w-0 flex-1 truncate">
                    {template.agent_name}
                    <span className="ml-1.5 text-xs text-muted-foreground">
                      {template.version}
                    </span>
                  </span>
                  {selected ? (
                    <Check aria-hidden className="size-4 text-primary" />
                  ) : null}
                </button>
              </li>
            )
          })}
        </ul>
      </PopoverContent>
    </Popover>
  )
}
