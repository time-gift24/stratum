"use client"

import { useCallback, useEffect, useMemo, useState } from "react"

import type { OntologyListPage } from "@/features/ontology-editor/types"
import { ApiError, type StratumApi } from "@/lib/stratum/api"
import { resolveOntologyApi } from "@/lib/stratum/mock-ontology-api"

// 契约（docs/ontology/API.md）：per_page 默认 20，sort 默认 -updated_at。
export const ONTOLOGY_LIST_PER_PAGE = 20
export const ONTOLOGY_LIST_SORT = "-updated_at"

export type OntologyListState =
  | { phase: "loading" }
  | { phase: "ready"; page: number; result: OntologyListPage }
  | { phase: "error"; page: number; message: string }

export type OntologyList = {
  state: OntologyListState
  api: StratumApi
  loadPage(page: number): void
  reload(): void
}

// 响应按请求 key 落盘：渲染时 key 不匹配即视为 loading，
// 翻页/重试不需要在 effect 里同步重置 state，过期响应也天然被忽略。
type SettledResult =
  | { key: string; ok: true; page: number; result: OntologyListPage }
  | { key: string; ok: false; page: number; message: string }

export function useOntologyList(options?: { api?: StratumApi }): OntologyList {
  const apiOption = options?.api
  const api = useMemo(() => resolveOntologyApi(apiOption), [apiOption])

  const [page, setPage] = useState(1)
  const [reloadVersion, setReloadVersion] = useState(0)
  const requestKey = `${page}:${reloadVersion}`
  const [settled, setSettled] = useState<SettledResult | null>(null)

  useEffect(() => {
    let cancelled = false
    const key = requestKey
    void api
      .listOntologies({
        page,
        perPage: ONTOLOGY_LIST_PER_PAGE,
        sort: ONTOLOGY_LIST_SORT,
      })
      .then((result) => {
        if (!cancelled) setSettled({ key, ok: true, page, result })
      })
      .catch((error: unknown) => {
        if (cancelled) return
        const message =
          error instanceof ApiError
            ? error.message
            : error instanceof Error
              ? error.message
              : "无法连接到 Stratum 后端"
        setSettled({ key, ok: false, page, message })
      })
    return () => {
      cancelled = true
    }
  }, [api, page, requestKey])

  const loadPage = useCallback((nextPage: number) => {
    setPage(Math.max(1, nextPage))
  }, [])

  const reload = useCallback(() => {
    setReloadVersion((version) => version + 1)
  }, [])

  const state: OntologyListState =
    settled === null || settled.key !== requestKey
      ? { phase: "loading" }
      : settled.ok
        ? { phase: "ready", page: settled.page, result: settled.result }
        : { phase: "error", page: settled.page, message: settled.message }

  return { state, api, loadPage, reload }
}
