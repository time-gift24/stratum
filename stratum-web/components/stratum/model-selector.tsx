"use client"

import { memo, useMemo, useState } from "react"
import { Check, ChevronDown } from "lucide-react"

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
import {
  modelDisplayName,
  type ModelDescriptor,
  type ThinkingLevel,
} from "@/lib/stratum/model-config"
import { cn } from "@/lib/utils"

/**
 * ModelSelector —— 数据驱动的模型选择器（assistant-ui model-selector 底稿的
 * 展示层 fork，不用它的 runtime/ModelContext）。布局：触发器（模型名 + Thinking
 * 等级 + chevron）→ popover 内搜索框 → provider 过滤 chips → 按 provider 分组的
 * 模型列表（选中打勾）→ 底部 Thinking 分段选择行。Thinking 等级完全由调用方
 * 从模型 schema 解析传入；无等级时不渲染 Thinking 行。
 */

const ALL_PROVIDERS = "全部"
const FALLBACK_PROVIDER = "其他"

type ModelEntry = {
  descriptor: ModelDescriptor
  id: string
  provider: string
  name: string
}

export const ModelSelector = memo(function ModelSelector({
  models,
  selectedModelId,
  onSelectModel,
  thinkingLevels,
  selectedThinkingLevel,
  onSelectThinkingLevel,
  loading = false,
  error = false,
  className,
}: {
  models: readonly ModelDescriptor[]
  /** 当前选中模型的完整 id（provider:name）；null 表示尚无可用选择 */
  selectedModelId: string | null
  onSelectModel: (descriptor: ModelDescriptor) => void
  /** 当前模型的 thinking 等级（schema 驱动）；空数组时隐藏 Thinking 行 */
  thinkingLevels: readonly ThinkingLevel[]
  /** 当前生效的 thinking 等级 id；null 表示未配置 */
  selectedThinkingLevel: string | null
  onSelectThinkingLevel: (level: string) => void
  loading?: boolean
  error?: boolean
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const [activeProvider, setActiveProvider] = useState<string | null>(null)

  const groups = useMemo(() => {
    const byProvider = new Map<string, ModelEntry[]>()
    for (const descriptor of models) {
      const display = modelDisplayName(descriptor.model)
      const provider = display.provider ?? FALLBACK_PROVIDER
      const entry: ModelEntry = {
        descriptor,
        id: descriptor.model,
        provider,
        name: display.model,
      }
      const group = byProvider.get(provider)
      if (group) group.push(entry)
      else byProvider.set(provider, [entry])
    }
    return [...byProvider.entries()].map(([provider, entries]) => ({
      provider,
      entries,
    }))
  }, [models])

  const visibleGroups =
    activeProvider === null
      ? groups
      : groups.filter((group) => group.provider === activeProvider)

  // 选中名直接从 id 推导（id 不在列表时显示原始 id，可接受）
  const selectedName = selectedModelId
    ? modelDisplayName(selectedModelId).model
    : null

  const activeLevelName =
    selectedThinkingLevel !== null && selectedThinkingLevel !== "disabled"
      ? (thinkingLevels.find((level) => level.id === selectedThinkingLevel)
          ?.name ?? null)
      : null

  const triggerLabel = loading
    ? "加载模型…"
    : error
      ? "模型不可用"
      : (selectedName ?? "选择模型")

  const triggerDisabled = loading || error || models.length === 0

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        disabled={triggerDisabled}
        aria-label={`模型：${triggerLabel}`}
        className={cn(
          "flex items-center gap-1 rounded-full px-2 py-1 font-sans text-sm text-muted-foreground transition-colors outline-none",
          "hover:bg-muted/60 hover:text-foreground",
          "focus-visible:ring-2 focus-visible:ring-ring/50",
          "disabled:cursor-not-allowed disabled:opacity-60",
          error && "text-destructive hover:text-destructive",
          className
        )}
      >
        <span className="max-w-40 truncate">{triggerLabel}</span>
        {activeLevelName ? (
          <span className="rounded-full bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
            {activeLevelName}
          </span>
        ) : null}
        <ChevronDown
          aria-hidden
          className={cn(
            "size-3.5 opacity-60 transition-transform duration-200",
            open && "rotate-180"
          )}
        />
      </PopoverTrigger>

      <PopoverContent
        side="top"
        align="end"
        sideOffset={8}
        className="w-72 overflow-hidden rounded-xl bg-popover p-0 shadow-lg"
      >
        <Command className="bg-transparent">
          <CommandInput autoFocus placeholder="搜索模型…" aria-label="搜索模型" />

          {groups.length > 1 ? (
            <div
              role="group"
              aria-label="按提供方过滤"
              className="flex flex-wrap items-center gap-1 px-2 pt-2"
            >
              {[ALL_PROVIDERS, ...groups.map((group) => group.provider)].map(
                (provider) => {
                  const active =
                    provider === ALL_PROVIDERS
                      ? activeProvider === null
                      : activeProvider === provider
                  return (
                    <button
                      key={provider}
                      type="button"
                      aria-pressed={active}
                      onClick={() =>
                        setActiveProvider(
                          provider === ALL_PROVIDERS ? null : provider
                        )
                      }
                      className={cn(
                        "rounded-full px-2 py-0.5 text-xs transition-colors outline-none",
                        "focus-visible:ring-2 focus-visible:ring-ring/50",
                        active
                          ? "bg-primary/15 text-primary"
                          : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                      )}
                    >
                      {provider}
                    </button>
                  )
                }
              )}
            </div>
          ) : null}

          <CommandList>
            <CommandEmpty className="px-3 py-6 text-center text-sm text-muted-foreground">
              没有匹配的模型
            </CommandEmpty>
            {visibleGroups.map((group) => (
              <CommandGroup key={group.provider} heading={group.provider}>
                {group.entries.map((entry) => (
                  <CommandItem
                    key={entry.id}
                    value={entry.id}
                    keywords={[group.provider, entry.name]}
                    onSelect={() => {
                      onSelectModel(entry.descriptor)
                      setOpen(false)
                    }}
                    className="gap-2 rounded-lg px-3 py-2"
                  >
                    <span className="min-w-0 flex-1 truncate text-sm">
                      {entry.name}
                    </span>
                    {entry.id === selectedModelId ? (
                      <Check aria-hidden className="size-4 text-primary" />
                    ) : null}
                  </CommandItem>
                ))}
              </CommandGroup>
            ))}
          </CommandList>

          {thinkingLevels.length > 0 ? (
            <div className="flex items-center justify-between gap-2 border-t border-border px-3 py-2">
              <span className="text-xs text-muted-foreground">Thinking</span>
              <div
                role="group"
                aria-label="Thinking 等级"
                className="flex items-center gap-0.5"
              >
                {thinkingLevels.map((level) => {
                  const active = level.id === selectedThinkingLevel
                  return (
                    <button
                      key={level.id}
                      type="button"
                      aria-pressed={active}
                      onClick={() => onSelectThinkingLevel(level.id)}
                      className={cn(
                        "rounded-md px-2 py-1 text-xs transition-colors outline-none",
                        "focus-visible:ring-2 focus-visible:ring-ring/50",
                        active
                          ? "bg-accent font-medium text-accent-foreground"
                          : "text-muted-foreground hover:text-foreground"
                      )}
                    >
                      {level.name}
                    </button>
                  )
                })}
              </div>
            </div>
          ) : null}
        </Command>
      </PopoverContent>
    </Popover>
  )
})
