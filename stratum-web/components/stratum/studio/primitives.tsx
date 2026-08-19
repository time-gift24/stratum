"use client"

import Link from "next/link"
import { usePathname } from "next/navigation"
import { cloneElement, isValidElement, useRef, useState } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import {
  ArrowLeft,
  ChevronRight,
  Cpu,
  LoaderCircle,
  Plug,
  Search,
  Trash2,
} from "lucide-react"
import type { LucideIcon } from "lucide-react"

import { Button, buttonVariants } from "@/components/ui/button"
import {
  Field as FieldRoot,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import { prefetchSettingsLanding } from "@/features/studio-management/settings-data"
import {
  MOTION_DURATION,
  MOTION_EASE,
  motionDuration,
  prefersReducedMotion,
} from "@/lib/motion"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)

export const controlClass =
  "rounded-lg border-border bg-card font-sans text-foreground shadow-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/25"

/** 选择器：ui/select 的 Studio 形态（h-9、w-full、token 化）。 */
export function StudioSelect({
  value,
  onChange,
  options,
  disabled,
  ariaLabel,
  "aria-invalid": ariaInvalid,
}: {
  value: string
  onChange(value: string): void
  options: readonly { value: string; label: string }[]
  disabled?: boolean
  ariaLabel?: string
  "aria-invalid"?: boolean
}) {
  return (
    <Select
      value={value}
      onValueChange={(next) => {
        if (next !== null) onChange(next)
      }}
      disabled={disabled}
    >
      <SelectTrigger
        aria-label={ariaLabel}
        aria-invalid={ariaInvalid}
        className={cn(
          controlClass,
          "h-9 min-h-11 w-full px-3 text-sm sm:min-h-9"
        )}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem
            key={option.value}
            value={option.value}
            className="min-h-11 text-sm"
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

export function PageShell({ children }: { children: React.ReactNode }) {
  return (
    <main className="min-h-svh bg-background px-4 pt-24 pb-16 font-sans text-foreground sm:px-6 sm:pt-28 lg:px-8">
      <div className="mx-auto w-full max-w-6xl">{children}</div>
    </main>
  )
}

export function PageHeader({
  title,
  children,
  backHref,
  backLabel = "返回仪表盘",
}: {
  title: string
  children?: React.ReactNode
  backHref?: string
  backLabel?: string
}) {
  return (
    <header className="mb-6 flex min-w-0 items-center gap-3 sm:mb-8">
      <div className="min-w-0 flex-1">
        {backHref ? (
          <Link
            href={backHref}
            className="mb-1 inline-flex min-h-11 items-center gap-1.5 rounded-md text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ArrowLeft aria-hidden className="size-4" />
            {backLabel}
          </Link>
        ) : null}
        <h1 className="truncate text-xl font-semibold tracking-[-0.02em] sm:text-2xl">
          {title}
        </h1>
      </div>
      {children}
    </header>
  )
}

/** 平面表单分组：legend + 描述 + 字段组，无卡片容器。 */
export function FormSection({
  title,
  description,
  children,
}: {
  title: string
  description?: string
  children: React.ReactNode
}) {
  return (
    <FieldSet>
      <FieldLegend className="text-base font-semibold tracking-[-0.01em]">
        {title}
      </FieldLegend>
      {description ? (
        <FieldDescription className="max-w-[65ch] text-sm leading-6">
          {description}
        </FieldDescription>
      ) : null}
      <FieldGroup className="mt-4 gap-5">{children}</FieldGroup>
    </FieldSet>
  )
}

/** 字段：label 包裹控件形成隐式关联，错误/说明在控件下方。 */
export function Field({
  label,
  error,
  hint,
  children,
}: {
  label: string
  error?: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <FieldRoot data-invalid={error ? true : undefined}>
      <FieldLabel className="w-full flex-col items-start gap-2 text-sm leading-normal font-medium select-text">
        {label}
        {error && isValidElement(children)
          ? cloneElement(
              children as React.ReactElement<Record<string, unknown>>,
              { "aria-invalid": true }
            )
          : children}
      </FieldLabel>
      {error ? (
        <FieldError className="text-sm">{error}</FieldError>
      ) : hint ? (
        <FieldDescription className="text-sm leading-5">
          {hint}
        </FieldDescription>
      ) : null}
    </FieldRoot>
  )
}

export function StudioInput(props: React.ComponentProps<typeof Input>) {
  return (
    <Input
      {...props}
      className={cn(controlClass, "h-11 sm:h-9", props.className)}
    />
  )
}

/**
 * 列表页搜索行：搜索框（放大镜即提交钮，也可回车）+ 右侧紧跟的图标化操作
 * （通常是新建入口）。仪表盘与本体列表共用，保持同一扫读语言。
 */
export function SearchRow({
  defaultValue,
  placeholder,
  onSearch,
  action,
}: {
  defaultValue: string
  placeholder: string
  onSearch(query: string): void
  action?: React.ReactNode
}) {
  return (
    <div className="mb-6 flex max-w-xl items-center gap-2">
      <form
        role="search"
        className="relative min-w-0 flex-1"
        onSubmit={(event) => {
          event.preventDefault()
          const data = new FormData(event.currentTarget)
          onSearch(String(data.get("q") ?? ""))
        }}
      >
        <Button
          type="submit"
          variant="ghost"
          size="icon"
          aria-label="搜索"
          className="absolute top-1/2 left-0 size-11 -translate-y-1/2 text-muted-foreground hover:text-foreground sm:left-1.5 sm:size-7"
        >
          <Search aria-hidden className="size-4" />
        </Button>
        <StudioInput
          key={defaultValue}
          name="q"
          defaultValue={defaultValue}
          placeholder={placeholder}
          aria-label={placeholder}
          className="pl-11 sm:pl-9"
        />
      </form>
      {action}
    </div>
  )
}

export function StudioTextarea(props: React.ComponentProps<typeof Textarea>) {
  return <Textarea {...props} className={cn(controlClass, props.className)} />
}

export function FormStatus({
  message,
  tone = "neutral",
}: {
  message: string | null
  tone?: "neutral" | "error" | "success"
}) {
  if (!message) return null
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={cn(
        "rounded-lg border px-3.5 py-2.5 text-sm leading-5",
        tone === "error" &&
          "border-destructive/35 bg-destructive/8 text-destructive",
        tone === "success" &&
          "border-accent bg-accent/45 text-accent-foreground",
        tone === "neutral" && "border-border bg-muted text-foreground"
      )}
    >
      {message}
    </div>
  )
}

export function BlockerList({
  blockers,
}: {
  blockers: readonly { resource_type: string; name: string; message?: string }[]
}) {
  if (blockers.length === 0) return null
  return (
    <div className="rounded-lg border border-destructive/35 bg-destructive/8 p-4 text-sm">
      <p className="mb-2 font-medium text-destructive">以下引用阻止了删除：</p>
      <ul className="space-y-1 text-foreground">
        {blockers.map((blocker) => (
          <li key={`${blocker.resource_type}:${blocker.name}`}>
            {blocker.resource_type} · {blocker.name}
            {blocker.message ? `：${blocker.message}` : ""}
          </li>
        ))}
      </ul>
    </div>
  )
}

export function SaveButton({
  saving,
  disabled,
  children = "保存",
}: {
  saving: boolean
  disabled?: boolean
  children?: React.ReactNode
}) {
  return (
    <Button
      type="submit"
      size="lg"
      className="min-h-11"
      disabled={disabled || saving}
    >
      {saving ? (
        <>
          <LoaderCircle
            aria-hidden
            className="animate-spin motion-reduce:animate-none"
          />
          保存中
        </>
      ) : (
        children
      )}
    </Button>
  )
}

/**
 * 删除操作：页面头部右上角的幽灵图标钮 + Popover 确认（解释 + 取消/确认）。
 * 危险操作应该可发现但不诱导——不再是页面底部的大红色区块。
 */
export function DeleteAction({
  resourceLabel,
  explanation,
  pending,
  disabled = false,
  onDelete,
}: {
  resourceLabel: string
  explanation: string
  pending: boolean
  disabled?: boolean
  onDelete: () => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        disabled={disabled}
        aria-label={`删除 ${resourceLabel}`}
        className="flex size-11 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors outline-none hover:bg-destructive/10 hover:text-destructive focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
      >
        <Trash2 aria-hidden className="size-4" />
      </PopoverTrigger>
      <PopoverContent align="end" className="grid w-80 gap-3 p-4 text-sm">
        <p className="font-medium text-destructive">删除 {resourceLabel}</p>
        <p className="leading-6 text-muted-foreground">{explanation}</p>
        <div className="flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            className="min-h-11"
            onClick={() => setOpen(false)}
          >
            取消
          </Button>
          <Button
            type="button"
            variant="destructive"
            className="min-h-11"
            disabled={pending}
            onClick={onDelete}
          >
            {pending ? "正在删除" : "确认删除"}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

const SETTINGS_ITEMS = [
  { key: "providers", label: "Provider", icon: Plug },
  { key: "models", label: "Model", icon: Cpu },
] as const

export type SettingsSection = (typeof SETTINGS_ITEMS)[number]["key"]

/** 父路径：/studio/settings/providers → /studio/settings。 */
function parentPath(pathname: string): string {
  const index = pathname.lastIndexOf("/")
  return index <= 0 ? "" : pathname.slice(0, index)
}

// 模块级接力状态：上一次选中项的视口矩形 + 所在路径。
// SettingsNav 在设置区内常驻不重挂载；只在跨区离开后再回来时重挂载，
// 靠它把选中底纹从上一位置滑过去而不是跳变。
let lastIndicator: {
  pathname: string
  rect: { left: number; top: number; width: number; height: number }
} | null = null

/**
 * 设置区导航（桌面左侧垂直 / 移动端顶部横排），由 settings 区共享 layout
 * （settings-chrome.tsx）挂载：区内任何导航（Provider ↔ Model 页签、下钻
 * 编辑器、返回列表）它都不重挂载，只有右侧内容变化。
 * 选中态由绝对定位 underlay 承载（accent 底 / dark primary tint）：点击即
 * 乐观滑动，不等路由提交；驻留期间 current 变化时从当前位置滑到目标；
 * 跨区重挂载时靠模块级矩形接力；prefers-reduced-motion 全部瞬时。
 */
export function SettingsNav({
  current,
  returnTo = "/studio",
}: {
  current: SettingsSection
  returnTo?: string
}) {
  const pathname = usePathname()
  const navRef = useRef<HTMLElement>(null)
  const indicatorRef = useRef<HTMLSpanElement>(null)
  const mountedRef = useRef(false)

  useGSAP(
    () => {
      const nav = navRef.current
      const indicator = indicatorRef.current
      if (!nav || !indicator) return

      const measure = () => {
        const active = nav.querySelector<HTMLElement>('[aria-current="page"]')
        if (!active) return null
        const navRect = nav.getBoundingClientRect()
        const rect = active.getBoundingClientRect()
        return {
          x: rect.left - navRect.left,
          y: rect.top - navRect.top,
          width: rect.width,
          height: rect.height,
          viewport: {
            left: rect.left,
            top: rect.top,
            width: rect.width,
            height: rect.height,
          },
        }
      }

      const target = measure()
      if (!target) return

      if (!mountedRef.current) {
        mountedRef.current = true
        // 挂载：同级页签接力滑动；跨区到达直接落位
        const from =
          lastIndicator !== null &&
          lastIndicator.pathname !== pathname &&
          parentPath(lastIndicator.pathname) === parentPath(pathname)
            ? lastIndicator
            : null
        if (from && !prefersReducedMotion()) {
          const navRect = nav.getBoundingClientRect()
          gsap.fromTo(
            indicator,
            {
              x: from.rect.left - navRect.left,
              y: from.rect.top - navRect.top,
              width: from.rect.width,
              height: from.rect.height,
            },
            {
              x: target.x,
              y: target.y,
              width: target.width,
              height: target.height,
              duration: MOTION_DURATION.fast,
              ease: MOTION_EASE.enter,
              overwrite: "auto",
            }
          )
        } else {
          gsap.set(indicator, {
            x: target.x,
            y: target.y,
            width: target.width,
            height: target.height,
          })
        }
      } else {
        // 驻留期间的页签切换：从当前位置（含乐观滑动后的位置）滑到目标
        gsap.to(indicator, {
          x: target.x,
          y: target.y,
          width: target.width,
          height: target.height,
          duration: motionDuration(MOTION_DURATION.fast),
          ease: MOTION_EASE.enter,
          overwrite: "auto",
        })
      }
      lastIndicator = { pathname, rect: target.viewport }

      const onResize = () => {
        const next = measure()
        if (!next) return
        gsap.set(indicator, {
          x: next.x,
          y: next.y,
          width: next.width,
          height: next.height,
        })
        lastIndicator = { pathname, rect: next.viewport }
      }
      window.addEventListener("resize", onResize)
      return () => window.removeEventListener("resize", onResize)
    },
    { scope: navRef, dependencies: [current] }
  )

  // 乐观滑动：点击当下就把 underlay 滑向目标项，不等 RSC 提交
  const previewSlide = (link: HTMLElement) => {
    const nav = navRef.current
    const indicator = indicatorRef.current
    if (!nav || !indicator || prefersReducedMotion()) return
    const navRect = nav.getBoundingClientRect()
    const rect = link.getBoundingClientRect()
    gsap.to(indicator, {
      x: rect.left - navRect.left,
      y: rect.top - navRect.top,
      width: rect.width,
      height: rect.height,
      duration: MOTION_DURATION.fast,
      ease: MOTION_EASE.enter,
      overwrite: "auto",
    })
  }

  return (
    <nav
      ref={navRef}
      aria-label="设置"
      className="relative flex gap-1 self-start lg:sticky lg:top-24 lg:flex-col"
    >
      <span
        ref={indicatorRef}
        aria-hidden
        className="pointer-events-none absolute top-0 left-0 rounded-lg bg-accent/60 dark:bg-primary/15"
      />
      {SETTINGS_ITEMS.map((item) => {
        const active = current === item.key
        const Icon = item.icon
        return (
          <Link
            key={item.key}
            href={`/studio/settings/${item.key}?${new URLSearchParams({ returnTo })}`}
            aria-current={active ? "page" : undefined}
            onClick={(event) => {
              if (!active) previewSlide(event.currentTarget)
            }}
            onPointerEnter={() => {
              // 悬停/聚焦即预热目标页签数据，切换时多数情况直接命中缓存，
              // 不再经过加载态
              if (!active) prefetchSettingsLanding(item.key)
            }}
            onFocus={() => {
              if (!active) prefetchSettingsLanding(item.key)
            }}
            className={cn(
              "relative flex h-11 flex-1 items-center gap-2.5 rounded-lg px-3 text-sm font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring lg:flex-none",
              active
                ? "text-accent-foreground dark:text-primary"
                : "text-muted-foreground hover:bg-muted/70 hover:text-foreground"
            )}
          >
            <Icon aria-hidden className="size-4 shrink-0" />
            {item.label}
          </Link>
        )
      })}
    </nav>
  )
}

/** 列表分页：总数摘要可选（如"共 N 项 · "），单页时不渲染。 */
export function Pagination({
  page,
  totalPages,
  onPageChange,
  label,
  summary = "",
}: {
  page: number
  totalPages: number
  onPageChange(page: number): void
  label: string
  summary?: string
}) {
  if (totalPages <= 1) return null
  return (
    <nav
      aria-label={label}
      className="mt-6 flex items-center justify-between gap-4"
    >
      <Button
        type="button"
        variant="outline"
        size="lg"
        className="min-h-11"
        disabled={page <= 1}
        onClick={() => onPageChange(page - 1)}
      >
        上一页
      </Button>
      <span className="text-sm text-muted-foreground">
        {summary}第 {page} / {totalPages} 页
      </span>
      <Button
        type="button"
        variant="outline"
        size="lg"
        className="min-h-11"
        disabled={page >= totalPages}
        onClick={() => onPageChange(page + 1)}
      >
        下一页
      </Button>
    </nav>
  )
}

/** 非列表整页/整区加载：编辑器等未知内容形态使用转圈指示。 */
export function LoadingState({ label }: { label: string }) {
  return (
    <div
      role="status"
      className="flex min-h-64 flex-col items-center justify-center gap-3 text-muted-foreground"
    >
      <LoaderCircle
        aria-hidden
        className="size-5 animate-spin motion-reduce:animate-none"
      />
      <span className="text-sm">{label}</span>
    </div>
  )
}

/** 真实状态 chip：只编码 API 返回的真实状态，不做装饰。 */
export function StatusChip({
  tone,
  children,
}: {
  tone: "ok" | "warn" | "neutral"
  children: React.ReactNode
}) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-[11px] leading-4 font-medium",
        tone === "ok" &&
          "bg-accent/50 text-accent-foreground dark:bg-primary/15 dark:text-primary",
        tone === "warn" && "bg-destructive/10 text-destructive",
        tone === "neutral" && "bg-muted text-muted-foreground"
      )}
    >
      {children}
    </span>
  )
}

/** 列表/资源加载失败：平面虚线面板（与空态同一语言），附重试入口。 */
export function ErrorState({
  title,
  message,
  onRetry,
}: {
  title: string
  message: string
  onRetry(): void
}) {
  return (
    <div
      role="alert"
      className="rounded-2xl border border-dashed border-destructive/40 p-7 sm:p-10"
    >
      <p className="font-medium text-destructive">{title}</p>
      <p className="mt-2 max-w-[65ch] text-sm leading-6 break-words text-muted-foreground">
        {message}
      </p>
      <Button
        type="button"
        variant="outline"
        size="lg"
        className="mt-4 min-h-11"
        onClick={onRetry}
      >
        重试
      </Button>
    </div>
  )
}

export type ResourceCardMeta = {
  icon: LucideIcon
  text: string
}

/** 资源不存在：不是错误死胡同，给出返回与新建两条出路。 */
export function NotFoundState({
  message,
  createHref,
  createLabel,
}: {
  message: string
  createHref: string
  createLabel: string
}) {
  return (
    <div className="rounded-2xl border border-dashed border-border p-7 sm:p-10">
      <p className="text-sm leading-6 text-muted-foreground">{message}</p>
      <div className="mt-5">
        <Link
          href={createHref}
          className={buttonVariants({ size: "lg", className: "min-h-11 px-4" })}
        >
          {createLabel}
        </Link>
      </div>
    </div>
  )
}

/**
 * 资源卡片：squircle 标识 + 名称 + 真实状态 chip + 虚线分隔的 meta 行。
 * 仪表盘与设置列表共用的扫读单元。`action` 为卡片内的次级操作
 * （如删除），渲染在链接之外的右侧区域。
 */
export function ResourceCard({
  href,
  title,
  leading,
  badge,
  meta,
  action,
}: {
  href: string
  title: string
  leading: React.ReactNode
  badge?: React.ReactNode
  meta: readonly ResourceCardMeta[]
  action?: React.ReactNode
}) {
  const body = (
    <>
      <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary font-mono text-sm font-semibold text-primary-foreground">
        {leading}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <p className="truncate text-sm font-semibold">{title}</p>
          {badge}
          {action ? null : (
            <ChevronRight
              aria-hidden
              className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground motion-reduce:transition-none"
            />
          )}
        </div>
        <div className="mt-2 grid">
          {meta.map((item, index) => (
            <div
              key={index}
              className={cn(
                "flex items-center gap-2 py-1.5 text-xs text-muted-foreground",
                index > 0 && "border-t border-dashed border-border"
              )}
            >
              <item.icon aria-hidden className="size-3.5 shrink-0" />
              <span className="truncate font-mono">{item.text}</span>
            </div>
          ))}
        </div>
      </div>
    </>
  )

  const cardClass =
    "group flex items-start gap-3.5 rounded-2xl border border-border bg-card p-4 transition-colors hover:border-foreground/25 motion-reduce:transition-none"

  if (action) {
    return (
      <div className={cardClass}>
        <Link
          href={href}
          title={title}
          className="flex min-w-0 flex-1 items-start gap-3.5 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {body}
        </Link>
        <div className="shrink-0">{action}</div>
      </div>
    )
  }

  return (
    <Link
      href={href}
      title={title}
      className={cn(
        cardClass,
        "outline-none focus-visible:ring-2 focus-visible:ring-ring"
      )}
    >
      {body}
    </Link>
  )
}
