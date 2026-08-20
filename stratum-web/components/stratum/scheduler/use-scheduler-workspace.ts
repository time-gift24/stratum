"use client"

import { useEffect, useState, type FormEvent } from "react"

import type { AgentTemplateView } from "@/lib/stratum/model-config"
import {
  ApiError,
  createStratumApi,
  STRATUM_API_BASE_URL,
  type Pagination,
  type ScheduleView,
} from "@/lib/stratum/api"

export type ScheduleLoadState =
  | {
      phase: "loading"
      schedules: readonly ScheduleView[]
      pagination: Pagination
    }
  | {
      phase: "ready"
      schedules: readonly ScheduleView[]
      pagination: Pagination
    }
  | {
      phase: "error"
      schedules: readonly ScheduleView[]
      pagination: Pagination
      message: string
    }

const PAGE_SIZE = 20
const EMPTY_PAGINATION: Pagination = { page: 1, per_page: PAGE_SIZE, total: 0 }

export function useSchedulerWorkspace() {
  const [page, setPage] = useState(1)
  const [loadState, setLoadState] = useState<ScheduleLoadState>({
    phase: "loading",
    schedules: [],
    pagination: EMPTY_PAGINATION,
  })
  const [templates, setTemplates] = useState<readonly AgentTemplateView[]>([])
  const [agentName, setAgentName] = useState("")
  const [cronExpression, setCronExpression] = useState("")
  const [submitting, setSubmitting] = useState(false)
  const [formError, setFormError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    void api.getAgentTemplates().then(
      (agentTemplates) => {
        if (cancelled) return
        setTemplates(agentTemplates)
        setAgentName(
          (current) => current || agentTemplates[0]?.agent_name || ""
        )
      },
      (error: unknown) => {
        if (!cancelled) setFormError(`无法加载 Agent：${errorMessage(error)}`)
      }
    )
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    void api.getSchedules({ page, perPage: PAGE_SIZE }).then(
      (schedules) => {
        if (!cancelled)
          setLoadState({
            phase: "ready",
            schedules: schedules.data,
            pagination: schedules.pagination,
          })
      },
      (error: unknown) => {
        if (!cancelled)
          setLoadState({
            phase: "error",
            schedules: [],
            pagination: { ...EMPTY_PAGINATION, page },
            message: errorMessage(error),
          })
      }
    )
    return () => {
      cancelled = true
    }
  }, [page])

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (agentName === "" || cronExpression.trim() === "") return
    setSubmitting(true)
    setFormError(null)
    try {
      const created = await createStratumApi({
        baseUrl: STRATUM_API_BASE_URL,
      }).createSchedule({
        agentName,
        cronExpression: cronExpression.trim(),
      })
      if (page === 1) {
        setLoadState((current) => ({
          phase: "ready",
          schedules: [
            created,
            ...current.schedules.filter(
              (schedule) => schedule.schedule_id !== created.schedule_id
            ),
          ].slice(0, current.pagination.per_page),
          pagination: {
            ...current.pagination,
            page: 1,
            total: current.pagination.total + 1,
          },
        }))
      } else {
        setLoadState({
          phase: "loading",
          schedules: [],
          pagination: { ...EMPTY_PAGINATION, page: 1 },
        })
        setPage(1)
      }
      setCronExpression("")
    } catch (error) {
      setFormError(errorMessage(error))
    } finally {
      setSubmitting(false)
    }
  }

  const showPreviousPage = () => {
    setLoadState((current) => ({
      phase: "loading",
      schedules: [],
      pagination: current.pagination,
    }))
    setPage((current) => Math.max(1, current - 1))
  }

  const showNextPage = () => {
    setLoadState((current) => ({
      phase: "loading",
      schedules: [],
      pagination: current.pagination,
    }))
    setPage((current) => current + 1)
  }

  return {
    page,
    loadState,
    templates,
    agentName,
    cronExpression,
    submitting,
    formError,
    setAgentName,
    setCronExpression,
    submit,
    showPreviousPage,
    showNextPage,
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message
  return error instanceof Error ? error.message : "请求失败，请稍后重试"
}
