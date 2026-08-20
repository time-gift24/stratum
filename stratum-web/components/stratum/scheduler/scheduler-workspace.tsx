"use client"

import type { FormEvent } from "react"
import Link from "next/link"
import { CalendarClock, ChevronRight, LoaderCircle, Plus } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { LoadingState } from "@/components/stratum/studio/primitives"
import type { AgentTemplateView } from "@/lib/stratum/model-config"
import type { ScheduleView } from "@/lib/stratum/api"
import {
  type ScheduleLoadState,
  useSchedulerWorkspace,
} from "./use-scheduler-workspace"

/**
 * DIRECTION CONTRACT —— /schedulers
 * THESIS: 把“何时由哪个 Agent 开始”压成一个清晰配置动作；拒绝监控大盘。
 * OWN-WORLD: 沿用中性工作台、绿 primary 与单层边框列表，cron 只在数据处用 mono。
 * STORY: 选择 Agent、写入 cron、确认下一次执行，再进入某条计划查看真实会话。
 * FIRST VIEWPORT: 左侧标题与约束，右侧同一基线上的配置表单；下方是可下钻列表。
 * FORM: Operate 模式的窄任务页，继承既有视觉系统，不引入新 token 或装饰动效。
 */

export function SchedulerWorkspace() {
  const workspace = useSchedulerWorkspace()

  return (
    <main className="min-h-svh px-4 pt-24 pb-16 font-sans sm:px-6 sm:pt-28">
      <div className="mx-auto w-full max-w-5xl">
        <header className="grid gap-6 border-b border-border pb-8 md:grid-cols-[minmax(0,0.8fr)_minmax(28rem,1.2fr)] md:items-end">
          <ScheduleIntroduction />
          <ScheduleForm
            templates={workspace.templates}
            agentName={workspace.agentName}
            cronExpression={workspace.cronExpression}
            submitting={workspace.submitting}
            error={workspace.formError}
            onAgentNameChange={workspace.setAgentName}
            onCronExpressionChange={workspace.setCronExpression}
            onSubmit={workspace.submit}
          />
        </header>

        <ScheduleList
          page={workspace.page}
          state={workspace.loadState}
          onPreviousPage={workspace.showPreviousPage}
          onNextPage={workspace.showNextPage}
        />
      </div>
    </main>
  )
}

function ScheduleIntroduction() {
  return (
    <div className="max-w-xl">
      <p className="mb-3 text-sm font-medium text-primary">计划任务</p>
      <h1 className="font-heading text-3xl tracking-[-0.025em] text-balance sm:text-4xl">
        让 Agent 按时开始工作
      </h1>
      <p className="mt-3 max-w-[60ch] text-sm leading-6 text-muted-foreground">
        Cron 按运行 Stratum
        的机器本地时区计算；下次执行时间按当前设备时区显示。每次触发都会创建一段独立、可恢复的对话。
      </p>
    </div>
  )
}

function ScheduleForm({
  templates,
  agentName,
  cronExpression,
  submitting,
  error,
  onAgentNameChange,
  onCronExpressionChange,
  onSubmit,
}: {
  templates: readonly AgentTemplateView[]
  agentName: string
  cronExpression: string
  submitting: boolean
  error: string | null
  onAgentNameChange: (value: string) => void
  onCronExpressionChange: (value: string) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
}) {
  return (
    <form
      onSubmit={onSubmit}
      className="grid gap-4 rounded-2xl bg-muted/55 p-4 sm:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)_auto] sm:items-end"
    >
      <label className="grid gap-2 text-sm font-medium">
        Agent
        <select
          value={agentName}
          onChange={(event) => onAgentNameChange(event.target.value)}
          disabled={templates.length === 0 || submitting}
          className="h-11 min-w-0 rounded-xl border border-input bg-background px-3 text-sm transition-colors outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {templates.length === 0 ? (
            <option value="">暂无可用 Agent</option>
          ) : null}
          {templates.map((template) => (
            <option key={template.agent_name} value={template.agent_name}>
              {template.agent_name}
            </option>
          ))}
        </select>
      </label>
      <label className="grid gap-2 text-sm font-medium">
        Cron 表达式
        <Input
          value={cronExpression}
          onChange={(event) => onCronExpressionChange(event.target.value)}
          placeholder="0 9 * * *"
          spellCheck={false}
          autoComplete="off"
          disabled={submitting}
          className="h-11 rounded-xl bg-background px-3 font-mono text-sm md:text-sm"
        />
      </label>
      <Button
        type="submit"
        size="lg"
        disabled={
          submitting || agentName === "" || cronExpression.trim() === ""
        }
        className="h-11 rounded-xl px-4 text-sm"
      >
        {submitting ? (
          <LoaderCircle
            aria-hidden
            className="animate-spin motion-reduce:animate-none"
          />
        ) : (
          <Plus aria-hidden />
        )}
        创建
      </Button>
      {error ? (
        <p role="alert" className="text-sm text-destructive sm:col-span-3">
          {error}
        </p>
      ) : null}
    </form>
  )
}

function ScheduleList({
  page,
  state,
  onPreviousPage,
  onNextPage,
}: {
  page: number
  state: ScheduleLoadState
  onPreviousPage: () => void
  onNextPage: () => void
}) {
  return (
    <section aria-labelledby="schedule-list-title" className="pt-10">
      <div className="mb-4 flex items-end justify-between gap-4">
        <div>
          <h2 id="schedule-list-title" className="text-lg font-semibold">
            已配置的计划
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">
            选择一条计划，查看它创建的历史会话。
          </p>
        </div>
        <span className="font-mono text-xs text-muted-foreground">
          {state.pagination.total}
        </span>
      </div>

      {state.phase === "loading" ? <LoadingState label="正在加载计划" /> : null}
      {state.phase === "error" ? (
        <div
          role="alert"
          className="rounded-2xl bg-destructive/10 px-5 py-4 text-sm text-destructive"
        >
          无法加载计划：{state.message}
        </div>
      ) : null}
      {state.phase === "ready" && state.schedules.length === 0 ? (
        <ScheduleEmptyState />
      ) : null}
      {state.schedules.length > 0 ? (
        <div className="overflow-hidden rounded-2xl border border-border bg-card">
          {state.schedules.map((schedule) => (
            <ScheduleRow key={schedule.schedule_id} schedule={schedule} />
          ))}
        </div>
      ) : null}
      {state.phase === "ready" &&
      state.pagination.total > state.pagination.per_page ? (
        <SchedulePagination
          page={page}
          total={state.pagination.total}
          perPage={state.pagination.per_page}
          onPreviousPage={onPreviousPage}
          onNextPage={onNextPage}
        />
      ) : null}
    </section>
  )
}

function ScheduleRow({ schedule }: { schedule: ScheduleView }) {
  return (
    <Link
      href={`/schedulers/${schedule.schedule_id}`}
      className="group grid min-h-20 gap-3 border-b border-border px-5 py-4 transition-colors last:border-b-0 hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-ring/40 focus-visible:outline-none focus-visible:ring-inset sm:grid-cols-[minmax(0,1fr)_minmax(13rem,0.7fr)_auto] sm:items-center"
    >
      <div className="min-w-0">
        <p className="truncate font-medium">{schedule.agent_name}</p>
        <code className="mt-1 block truncate text-xs text-muted-foreground">
          {schedule.cron_expression}
        </code>
      </div>
      <div className="min-w-0 text-sm">
        <p className="text-xs text-muted-foreground">下次执行（设备时区）</p>
        <time dateTime={schedule.next_run_at} className="mt-1 block">
          {formatDateTime(schedule.next_run_at)}
        </time>
      </div>
      <ChevronRight
        aria-hidden
        className="hidden size-4 text-muted-foreground transition-transform group-hover:translate-x-0.5 sm:block"
      />
    </Link>
  )
}

function ScheduleEmptyState() {
  return (
    <div className="flex min-h-56 flex-col items-center justify-center rounded-2xl border border-dashed border-border px-6 text-center">
      <CalendarClock
        aria-hidden
        className="mb-4 size-6 text-muted-foreground"
      />
      <p className="font-medium">还没有计划任务</p>
      <p className="mt-1 max-w-md text-sm leading-6 text-muted-foreground">
        从上方选择 Agent 并填写 cron
        表达式。首次触发后，这里会出现可下钻的会话记录。
      </p>
    </div>
  )
}

function SchedulePagination({
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

function formatDateTime(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  }).format(date)
}
