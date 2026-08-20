"use client"

import Link from "next/link"
import {
  ArrowLeft,
  CalendarClock,
  ChevronRight,
  MessageCircle,
  RefreshCw,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { LoadingState } from "@/components/stratum/studio/primitives"
import type {
  Pagination,
  ScheduleSessionView,
  ScheduleView,
} from "@/lib/stratum/api"
import { cn } from "@/lib/utils"
import { useScheduleHistory } from "./use-schedule-history"

/**
 * DIRECTION CONTRACT —— /schedulers/[scheduleId]
 * THESIS: 每次触发都是一段可核查的真实对话；拒绝把执行记录做成指标面板。
 * OWN-WORLD: 继承计划列表的单层行式结构，状态只用语义色与明确文字表达。
 * STORY: 先确认计划身份与下次执行，再按时间进入某次 Session 的完整对话。
 * FIRST VIEWPORT: 返回入口、计划摘要、紧接其后的会话时间线，不放无关统计。
 * FORM: Operate 模式的二级详情页；对话链接携带返回上下文，形成闭合下钻路径。
 */

export function ScheduleHistory({ scheduleId }: { scheduleId: string }) {
  const history = useScheduleHistory(scheduleId)

  return (
    <main className="min-h-svh px-4 pt-24 pb-16 font-sans sm:px-6 sm:pt-28">
      <div className="mx-auto w-full max-w-4xl">
        <Link
          href="/schedulers"
          className="mb-8 inline-flex min-h-11 items-center gap-2 rounded-xl px-3 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none"
        >
          <ArrowLeft aria-hidden className="size-4" />
          返回计划任务
        </Link>

        {history.state.phase === "loading" ? (
          <LoadingState label="正在加载会话历史" />
        ) : null}
        {history.state.phase === "error" ? (
          <div
            role="alert"
            className="rounded-2xl bg-destructive/10 px-5 py-4 text-sm text-destructive"
          >
            无法加载会话历史：{history.state.message}
          </div>
        ) : null}
        {history.state.phase === "ready" ? (
          <HistoryContent
            scheduleId={scheduleId}
            page={history.page}
            schedule={history.state.schedule}
            sessions={history.state.sessions}
            pagination={history.state.pagination}
            onRefresh={history.refresh}
            onPreviousPage={history.showPreviousPage}
            onNextPage={history.showNextPage}
          />
        ) : null}
      </div>
    </main>
  )
}

function HistoryContent({
  scheduleId,
  page,
  schedule,
  sessions,
  pagination,
  onRefresh,
  onPreviousPage,
  onNextPage,
}: {
  scheduleId: string
  page: number
  schedule: ScheduleView
  sessions: readonly ScheduleSessionView[]
  pagination: Pagination
  onRefresh: () => void
  onPreviousPage: () => void
  onNextPage: () => void
}) {
  return (
    <>
      <ScheduleSummary schedule={schedule} />
      <SessionList
        scheduleId={scheduleId}
        page={page}
        sessions={sessions}
        pagination={pagination}
        onRefresh={onRefresh}
        onPreviousPage={onPreviousPage}
        onNextPage={onNextPage}
      />
    </>
  )
}

function ScheduleSummary({ schedule }: { schedule: ScheduleView }) {
  return (
    <header className="grid gap-6 border-b border-border pb-8 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
      <div className="min-w-0">
        <p className="mb-3 text-sm font-medium text-primary">计划详情</p>
        <h1 className="truncate font-heading text-3xl tracking-[-0.025em] sm:text-4xl">
          {schedule.agent_name}
        </h1>
        <code className="mt-3 block text-sm text-muted-foreground">
          {schedule.cron_expression}
        </code>
      </div>
      <div className="sm:text-right">
        <p className="text-xs text-muted-foreground">下次执行（设备时区）</p>
        <time
          dateTime={schedule.next_run_at}
          className="mt-1 block text-sm font-medium"
        >
          {formatDateTime(schedule.next_run_at)}
        </time>
      </div>
    </header>
  )
}

function SessionList({
  scheduleId,
  page,
  sessions,
  pagination,
  onRefresh,
  onPreviousPage,
  onNextPage,
}: {
  scheduleId: string
  page: number
  sessions: readonly ScheduleSessionView[]
  pagination: Pagination
  onRefresh: () => void
  onPreviousPage: () => void
  onNextPage: () => void
}) {
  return (
    <section aria-labelledby="session-list-title" className="pt-10">
      <div className="mb-4 flex items-end justify-between gap-4">
        <div>
          <h2 id="session-list-title" className="text-lg font-semibold">
            历史会话
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">
            每一项对应一次独立的计划触发。
          </p>
        </div>
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs text-muted-foreground">
            {pagination.total}
          </span>
          <Button
            type="button"
            variant="ghost"
            size="lg"
            onClick={onRefresh}
            className="h-11 rounded-xl px-3 text-muted-foreground"
          >
            <RefreshCw aria-hidden className="size-4" />
            刷新
          </Button>
        </div>
      </div>

      {sessions.length === 0 ? (
        <SessionEmptyState />
      ) : (
        <div className="overflow-hidden rounded-2xl border border-border bg-card">
          {sessions.map((session) => (
            <SessionEntry
              key={session.session_id}
              scheduleId={scheduleId}
              session={session}
            />
          ))}
        </div>
      )}

      {pagination.total > pagination.per_page ? (
        <SessionPagination
          page={page}
          total={pagination.total}
          perPage={pagination.per_page}
          onPreviousPage={onPreviousPage}
          onNextPage={onNextPage}
        />
      ) : null}
    </section>
  )
}

function SessionEntry({
  scheduleId,
  session,
}: {
  scheduleId: string
  session: ScheduleSessionView
}) {
  const content = <SessionRow session={session} />
  return session.conversation_available && session.agent_runtime_id !== null ? (
    <Link
      href={`/conversation?agent_runtime_id=${encodeURIComponent(session.agent_runtime_id)}&schedule_id=${encodeURIComponent(scheduleId)}`}
      className="group block border-b border-border transition-colors last:border-b-0 hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none focus-visible:ring-inset"
    >
      {content}
    </Link>
  ) : (
    <div className="border-b border-border last:border-b-0">{content}</div>
  )
}

function SessionRow({ session }: { session: ScheduleSessionView }) {
  const status = STATUS[session.status]
  return (
    <div className="grid min-h-20 gap-3 px-5 py-4 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-center">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <MessageCircle aria-hidden className="size-4 text-muted-foreground" />
          <time dateTime={session.triggered_at} className="font-medium">
            {formatDateTime(session.triggered_at)}
          </time>
        </div>
        <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
          Session {shortId(session.session_id)}
        </p>
      </div>
      <span
        className={cn(
          "inline-flex w-fit items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium",
          status.className
        )}
      >
        <span aria-hidden className="size-1.5 rounded-full bg-current" />
        {status.label}
      </span>
      {session.conversation_available ? (
        <ChevronRight
          aria-hidden
          className="hidden size-4 text-muted-foreground transition-transform group-hover:translate-x-0.5 sm:block"
        />
      ) : (
        <span className="text-xs text-muted-foreground">暂无对话</span>
      )}
    </div>
  )
}

function SessionEmptyState() {
  return (
    <div className="flex min-h-56 flex-col items-center justify-center rounded-2xl border border-dashed border-border px-6 text-center">
      <CalendarClock
        aria-hidden
        className="mb-4 size-6 text-muted-foreground"
      />
      <p className="font-medium">还没有触发记录</p>
      <p className="mt-1 max-w-md text-sm leading-6 text-muted-foreground">
        到达下一次执行时间后，Stratum 会在这里写入新的 Session。
      </p>
    </div>
  )
}

function SessionPagination({
  page,
  total,
  perPage,
  onPreviousPage,
  onNextPage,
}: {
  page: number
  total: number
  perPage: number
  onPreviousPage: () => void
  onNextPage: () => void
}) {
  return (
    <div className="mt-5 flex items-center justify-end gap-2">
      <Button
        type="button"
        variant="outline"
        size="lg"
        disabled={page === 1}
        onClick={onPreviousPage}
        className="h-11 rounded-xl"
      >
        上一页
      </Button>
      <span className="min-w-16 text-center font-mono text-xs text-muted-foreground">
        {page} / {Math.ceil(total / perPage)}
      </span>
      <Button
        type="button"
        variant="outline"
        size="lg"
        disabled={page * perPage >= total}
        onClick={onNextPage}
        className="h-11 rounded-xl"
      >
        下一页
      </Button>
    </div>
  )
}

const STATUS: Record<
  ScheduleSessionView["status"],
  { label: string; className: string }
> = {
  starting: { label: "启动中", className: "bg-port-image/10 text-port-image" },
  running: { label: "执行中", className: "bg-port-image/10 text-port-image" },
  finished: { label: "已完成", className: "bg-muted text-foreground" },
  failed: { label: "失败", className: "bg-destructive/10 text-destructive" },
  cancelled: {
    label: "已取消",
    className: "bg-muted text-muted-foreground",
  },
}

function shortId(value: string): string {
  return value.length > 12 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value
}

function formatDateTime(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    timeZoneName: "short",
  }).format(date)
}
