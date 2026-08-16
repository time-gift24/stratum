"use client"

import Link from "next/link"
import { usePathname } from "next/navigation"
import { cloneElement, isValidElement, useRef } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import {
  ArrowLeft,
  ChevronRight,
  Cpu,
  LoaderCircle,
  Plug,
  Search,
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
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
        className={cn(controlClass, "h-9 w-full px-3 text-sm")}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem
            key={option.value}
            value={option.value}
            className="text-sm"
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
    <main className="min-h-svh bg-background px-4 pt-20 pb-16 font-sans text-foreground sm:px-6 lg:px-8">
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
            className="mb-1 inline-flex h-8 items-center gap-1.5 rounded-md text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
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
    <Input {...props} className={cn(controlClass, "h-9", props.className)} />
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
        <button
          type="submit"
          aria-label="搜索"
          className="absolute top-1/2 left-1.5 flex size-7 -translate-y-1/2 items-center justify-center rounded-md text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Search aria-hidden className="size-4" />
        </button>
        <StudioInput
          key={defaultValue}
          name="q"
          defaultValue={defaultValue}
          placeholder={placeholder}
          aria-label={placeholder}
          className="pl-9"
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
    <Button type="submit" size="lg" disabled={disabled || saving}>
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

export function InlineDelete({
  resourceLabel,
  explanation,
  pending,
  onDelete,
}: {
  resourceLabel: string
  explanation: string
  pending: boolean
  onDelete: () => void
}) {
  const detailsId = `delete-${resourceLabel.replace(/[^a-zA-Z0-9_-]/g, "-")}`
  return (
    <details className="group rounded-lg border border-destructive/30 px-4 py-3">
      <summary
        aria-controls={detailsId}
        className="flex min-h-9 cursor-pointer list-none items-center rounded-md text-sm font-medium text-destructive outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        删除 {resourceLabel}
      </summary>
      <div id={detailsId} className="grid gap-3 pt-2 pb-1">
        <p className="max-w-[70ch] text-sm leading-6 text-muted-foreground">
          {explanation}
        </p>
        <Button
          type="button"
          variant="destructive"
          size="lg"
          className="w-fit"
          disabled={pending}
          onClick={onDelete}
        >
          {pending ? "正在删除" : "确认删除"}
        </Button>
      </div>
    </details>
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
// SettingsShell 随页面重挂载，靠它在页签切换时把选中底纹滑过去而不是跳变。
let lastIndicator: {
  pathname: string
  rect: { left: number; top: number; width: number; height: number }
} | null = null

/**
 * 设置区外壳：左侧垂直导航（桌面）/ 顶部横排（移动端）+ 右侧内容。
 * 选中态与全站 rail 同一语言（accent 底 / dark primary tint），
 * 由绝对定位 underlay 承载；同级页签切换时 underlay 从上一位置滑动到位，
 * 内容区做一次快速淡入上浮（prefers-reduced-motion 时全部瞬时）。
 */
export function SettingsShell({
  current,
  returnTo = "/studio",
  children,
}: {
  current: SettingsSection
  returnTo?: string
  children: React.ReactNode
}) {
  const pathname = usePathname()
  const navRef = useRef<HTMLElement>(null)
  const indicatorRef = useRef<HTMLSpanElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)

  useGSAP(
    () => {
      const nav = navRef.current
      const indicator = indicatorRef.current
      const content = contentRef.current
      if (!nav || !indicator || !content) return

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

      const place = (from: typeof lastIndicator) => {
        const target = measure()
        if (!target) return
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
        lastIndicator = { pathname, rect: target.viewport }
      }

      // 只有同级页签切换（父路径相同）才接力滑动；跨区到达直接落位
      const from =
        lastIndicator !== null &&
        parentPath(lastIndicator.pathname) === parentPath(pathname)
          ? lastIndicator
          : null
      place(from)

      const onResize = () => place(null)
      window.addEventListener("resize", onResize)

      // 内容区进场：快速淡入 + 轻微上浮，软化页签切换的内容替换
      gsap.fromTo(
        content,
        { opacity: 0, y: 6 },
        {
          opacity: 1,
          y: 0,
          duration: motionDuration(MOTION_DURATION.fast),
          ease: MOTION_EASE.enter,
          clearProps: "transform,opacity",
        }
      )

      return () => window.removeEventListener("resize", onResize)
    },
    { scope: navRef }
  )

  return (
    <div className="grid gap-6 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-8">
      <nav
        ref={navRef}
        aria-label="设置"
        className="relative flex gap-1 self-start lg:sticky lg:top-20 lg:flex-col"
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
              className={cn(
                "relative flex h-9 flex-1 items-center gap-2.5 rounded-lg px-3 text-sm font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring lg:flex-none",
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
      <div ref={contentRef} className="min-w-0">
        {children}
      </div>
    </div>
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
        disabled={page >= totalPages}
        onClick={() => onPageChange(page + 1)}
      >
        下一页
      </Button>
    </nav>
  )
}

/** 整页/整区加载：转圈指示。骨架只用于卡片框架已在、局部内容在加载的场景。 */
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
        className="mt-4"
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
          className={buttonVariants({ size: "lg" })}
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
