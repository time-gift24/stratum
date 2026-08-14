"use client"

import { useState } from "react"
import { Check, ChevronDown, X } from "lucide-react"

import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { controlClass } from "@/components/stratum/studio/primitives"
import { cn } from "@/lib/utils"
import type { ToolView } from "@/lib/stratum/api"

const KIND_LABEL: Record<ToolView["kind"], string> = {
  read: "只读",
  write: "写入",
}

const DANGER_LABEL: Record<ToolView["danger_level"], string> = {
  low: "低风险",
  medium: "中风险",
  high: "高风险",
}

/**
 * Agent 工具多选：cmdk 可搜索下拉 + 已选 chip 列表。
 * 目录来自 GET /v1/tools（host 实际可注册的工具）；definition 里
 * 已存在但不在目录中的名称仍显示为 chip，可移除、不可新增。
 */
export function ToolsSelect({
  catalog,
  value,
  onChange,
  disabled,
  "aria-invalid": ariaInvalid,
}: {
  catalog: readonly ToolView[]
  value: readonly string[]
  onChange: (tools: string[]) => void
  disabled?: boolean
  "aria-invalid"?: boolean
}) {
  const [open, setOpen] = useState(false)

  const toggle = (name: string) => {
    onChange(
      value.includes(name)
        ? value.filter((tool) => tool !== name)
        : [...value, name]
    )
  }

  return (
    <div className="grid gap-3">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          disabled={disabled}
          aria-label="选择工具"
          aria-invalid={ariaInvalid}
          className={cn(
            controlClass,
            "flex h-9 w-full items-center justify-between rounded-lg border px-3 text-sm outline-none disabled:cursor-not-allowed disabled:opacity-50"
          )}
        >
          <span className={cn(value.length === 0 && "text-muted-foreground")}>
            {value.length === 0 ? "选择工具" : `已选 ${value.length} 个工具`}
          </span>
          <ChevronDown
            aria-hidden
            className={cn(
              "size-4 text-muted-foreground transition-transform motion-reduce:transition-none",
              open && "rotate-180"
            )}
          />
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-80 overflow-hidden rounded-xl bg-popover p-0 shadow-lg"
        >
          <Command className="bg-transparent">
            <CommandInput placeholder="搜索工具" aria-label="搜索工具" />
            <CommandList>
              <CommandEmpty className="px-3 py-6 text-center text-sm text-muted-foreground">
                没有匹配的工具
              </CommandEmpty>
              <CommandGroup>
                {catalog.map((tool) => {
                  const selected = value.includes(tool.name)
                  return (
                    <CommandItem
                      key={tool.name}
                      value={tool.name}
                      keywords={[tool.description]}
                      onSelect={() => toggle(tool.name)}
                      className="gap-2 rounded-lg px-3 py-2"
                    >
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-mono text-sm">
                          {tool.name}
                        </span>
                        <span className="block truncate text-xs text-muted-foreground">
                          {tool.description}
                        </span>
                      </span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {KIND_LABEL[tool.kind]} · {DANGER_LABEL[tool.danger_level]}
                      </span>
                      {selected ? (
                        <Check
                          aria-hidden
                          className="size-4 shrink-0 text-accent-foreground dark:text-primary"
                        />
                      ) : null}
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {value.length > 0 ? (
        <ul className="flex flex-wrap gap-2" aria-label="已选工具">
          {value.map((name) => (
            <li
              key={name}
              className="flex items-center gap-1.5 rounded-lg border border-border bg-muted px-2.5 py-1.5 font-mono text-xs"
            >
              {name}
              <button
                type="button"
                aria-label={`移除 ${name}`}
                onClick={() => toggle(name)}
                className="rounded p-0.5 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                <X aria-hidden className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}
