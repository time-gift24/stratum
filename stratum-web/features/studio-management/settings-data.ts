"use client"

/**
 * 设置区 Provider 列表数据：全量拉取（上限 100）后前端过滤分页，
 * 供 Provider 列表页加载与悬停预取共用。
 */

import { studioApi } from "@/features/studio-management/client"
import type { PageEnvelope, ProviderView } from "@/lib/stratum/api"

export const SETTINGS_PER_PAGE = 12

export type ProvidersPageData = {
  result: PageEnvelope<ProviderView>
  hasResources: boolean
}

export async function fetchProvidersPage(
  page: number,
  query: string
): Promise<ProvidersPageData> {
  const all = await studioApi.listProviders({ page: 1, perPage: 100 })
  const normalized = query.toLocaleLowerCase()
  const filtered = all.data.filter(
    (provider) =>
      normalized === "" ||
      provider.provider.toLocaleLowerCase().includes(normalized)
  )
  const start = (page - 1) * SETTINGS_PER_PAGE
  return {
    result: {
      data: filtered.slice(start, start + SETTINGS_PER_PAGE),
      pagination: {
        page,
        per_page: SETTINGS_PER_PAGE,
        total: filtered.length,
      },
    },
    hasResources: all.pagination.total > 0,
  }
}
