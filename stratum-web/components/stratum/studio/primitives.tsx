"use client"

import Link from "next/link"
import { ArrowLeft, ChevronRight, LoaderCircle, Settings } from "lucide-react"

import { Button, buttonVariants } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

export const controlClass =
  "border-border bg-card text-foreground shadow-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/25"

export function StudioPage({ children }: { children: React.ReactNode }) {
  return (
    <main className="min-h-svh bg-background px-4 pt-24 pb-16 text-foreground sm:px-6 sm:pt-28 lg:px-8">
      <div className="mx-auto w-full max-w-6xl">{children}</div>
    </main>
  )
}

export function StudioHeader({
  title,
  children,
  backHref,
  backLabel = "返回 Studio",
  settings = false,
  settingsHref = "/studio/settings/providers",
}: {
  title: string
  children?: React.ReactNode
  backHref?: string
  backLabel?: string
  settings?: boolean
  settingsHref?: string
}) {
  return (
    <header className="mb-8 flex min-w-0 items-center gap-3 sm:mb-10">
      <div className="min-w-0 flex-1">
        {backHref ? (
          <Link
            href={backHref}
            className="mb-2 inline-flex min-h-11 items-center gap-2 rounded-lg text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ArrowLeft aria-hidden className="size-4" />
            {backLabel}
          </Link>
        ) : null}
        <h1 className="truncate font-sans text-2xl font-semibold tracking-[-0.02em] sm:text-3xl">
          {title}
        </h1>
      </div>
      {children}
      {settings ? (
        <Link
          href={settingsHref}
          aria-label="设置"
          className={buttonVariants({
            variant: "outline",
            size: "icon",
            className:
              "size-11 shrink-0 rounded-xl border-border bg-card shadow-[0_5px_14px_color-mix(in_oklab,var(--foreground)_8%,transparent)] hover:bg-muted focus-visible:ring-ring",
          })}
        >
          <Settings aria-hidden className="size-[18px]" />
        </Link>
      ) : null}
    </header>
  )
}

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
    <label className="grid gap-2 text-sm font-medium">
      <span>{label}</span>
      {children}
      {error ? (
        <span className="text-sm font-normal text-destructive" role="alert">
          {error}
        </span>
      ) : hint ? (
        <span className="text-sm leading-5 font-normal text-muted-foreground">
          {hint}
        </span>
      ) : null}
    </label>
  )
}

export function StudioInput(props: React.ComponentProps<typeof Input>) {
  return <Input {...props} className={cn(controlClass, props.className)} />
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
        "rounded-xl border px-4 py-3 text-sm leading-5",
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
    <div className="rounded-xl border border-destructive/35 bg-destructive/8 p-4 text-sm">
      <p className="mb-2 font-medium text-destructive">以下引用阻止了删除：</p>
      <ul className="space-y-1 text-foreground">
        {blockers.map((blocker) => (
          <li key={`${blocker.resource_type}:${blocker.name}`}>
            {blocker.resource_type} · {blocker.name}
            {blocker.message ? ` — ${blocker.message}` : ""}
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
      disabled={disabled || saving}
      className="min-h-11 min-w-24 rounded-xl bg-primary text-primary-foreground hover:bg-primary/90"
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
    <details className="group rounded-xl border border-destructive/30 bg-card p-4">
      <summary className="min-h-11 cursor-pointer list-none rounded-lg px-2 py-2 text-sm font-medium text-destructive outline-none focus-visible:ring-2 focus-visible:ring-ring">
        删除 {resourceLabel}
      </summary>
      <div
        id={detailsId}
        className="mt-3 grid gap-3 border-t border-border pt-4"
      >
        <p className="max-w-[70ch] text-sm leading-6 text-muted-foreground">
          {explanation}
        </p>
        <Button
          type="button"
          variant="destructive"
          className="min-h-11 w-fit rounded-xl"
          disabled={pending}
          onClick={onDelete}
        >
          {pending ? "正在删除" : "确认删除"}
        </Button>
      </div>
    </details>
  )
}

export function SettingsNav({
  current,
  returnTo = "/studio",
}: {
  current: "providers" | "models"
  returnTo?: string
}) {
  return (
    <nav
      aria-label="Studio 设置"
      className="relative mb-8 grid w-full max-w-sm grid-cols-2 rounded-xl bg-muted p-1"
    >
      <span
        aria-hidden
        data-current={current}
        className="absolute inset-y-1 left-1 w-[calc(50%-0.25rem)] rounded-lg bg-accent shadow-[0_2px_7px_color-mix(in_oklab,var(--foreground)_7%,transparent)] transition-transform duration-200 ease-out data-[current=models]:translate-x-full motion-reduce:transition-none"
      />
      {(["providers", "models"] as const).map((item) => (
        <Link
          key={item}
          href={`/studio/settings/${item}?${new URLSearchParams({ returnTo })}`}
          aria-current={current === item ? "page" : undefined}
          className="relative z-10 flex min-h-11 items-center justify-center rounded-lg px-4 text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {item === "providers" ? "Provider" : "Model"}
        </Link>
      ))}
    </nav>
  )
}

export function ListRow({
  href,
  title,
  meta,
  children,
}: {
  href: string
  title: string
  meta: string
  children?: React.ReactNode
}) {
  return (
    <Link
      href={href}
      title={title}
      className="group flex min-h-16 items-center gap-4 border-b border-border px-1 py-4 outline-none last:border-b-0 hover:bg-muted/55 focus-visible:rounded-lg focus-visible:ring-2 focus-visible:ring-ring sm:px-3"
    >
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium">{title}</p>
        <p className="mt-1 truncate text-sm text-muted-foreground">{meta}</p>
      </div>
      {children}
      <ChevronRight
        aria-hidden
        className="size-4 shrink-0 text-muted-foreground"
      />
    </Link>
  )
}
