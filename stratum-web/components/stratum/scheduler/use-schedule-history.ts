"use client"

import { useEffect, useState } from "react"

import {
  ApiError,
  createStratumApi,
  STRATUM_API_BASE_URL,
  type Pagination,
  type ScheduleSessionView,
  type ScheduleView,
} from "@/lib/stratum/api"

export type HistoryState =
  | { phase: "loading" }
  | {
      phase: "ready"
      schedule: ScheduleView
      sessions: readonly ScheduleSessionView[]
      pagination: Pagination
    }
  | { phase: "error"; message: string }

const PAGE_SIZE = 20

type ScheduleResource =
  | { phase: "loading" }
  | { phase: "ready"; schedule: ScheduleView }
  | { phase: "error"; message: string }

type SessionsResource =
  | { phase: "loading" }
  | {
      phase: "ready"
      sessions: readonly ScheduleSessionView[]
      pagination: Pagination
    }
  | { phase: "error"; message: string }

export function useScheduleHistory(scheduleId: string) {
  const [page, setPage] = useState(1)
  const [refreshVersion, setRefreshVersion] = useState(0)
  const [scheduleResource, setScheduleResource] = useState<ScheduleResource>({
    phase: "loading",
  })
  const [sessionsResource, setSessionsResource] = useState<SessionsResource>({
    phase: "loading",
  })

  useEffect(() => {
    let cancelled = false
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    void api.getSchedule(scheduleId).then(
      (schedule) => {
        if (!cancelled) setScheduleResource({ phase: "ready", schedule })
      },
      (error: unknown) => {
        if (!cancelled)
          setScheduleResource({
            phase: "error",
            message: errorMessage(error),
          })
      }
    )
    return () => {
      cancelled = true
    }
  }, [scheduleId])

  useEffect(() => {
    let cancelled = false
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    void api.getScheduleSessions(scheduleId, { page, perPage: PAGE_SIZE }).then(
      (sessions) => {
        if (!cancelled)
          setSessionsResource({
            phase: "ready",
            sessions: sessions.data,
            pagination: sessions.pagination,
          })
      },
      (error: unknown) => {
        if (!cancelled)
          setSessionsResource({
            phase: "error",
            message: errorMessage(error),
          })
      }
    )
    return () => {
      cancelled = true
    }
  }, [page, refreshVersion, scheduleId])

  useEffect(() => {
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible")
        setRefreshVersion((version) => version + 1)
    }
    const timer = window.setInterval(refreshWhenVisible, 5_000)
    document.addEventListener("visibilitychange", refreshWhenVisible)
    return () => {
      window.clearInterval(timer)
      document.removeEventListener("visibilitychange", refreshWhenVisible)
    }
  }, [])

  const refresh = () => setRefreshVersion((version) => version + 1)

  const showPreviousPage = () => {
    setSessionsResource({ phase: "loading" })
    setPage((current) => Math.max(1, current - 1))
  }

  const showNextPage = () => {
    setSessionsResource({ phase: "loading" })
    setPage((current) => current + 1)
  }

  const state = historyState(scheduleResource, sessionsResource)

  return {
    page,
    state,
    refresh,
    showPreviousPage,
    showNextPage,
  }
}

function historyState(
  schedule: ScheduleResource,
  sessions: SessionsResource
): HistoryState {
  if (schedule.phase === "error")
    return { phase: "error", message: schedule.message }
  if (sessions.phase === "error")
    return { phase: "error", message: sessions.message }
  if (schedule.phase === "loading" || sessions.phase === "loading") {
    return { phase: "loading" }
  }
  return {
    phase: "ready",
    schedule: schedule.schedule,
    sessions: sessions.sessions,
    pagination: sessions.pagination,
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message
  return error instanceof Error ? error.message : "请求失败，请稍后重试"
}
