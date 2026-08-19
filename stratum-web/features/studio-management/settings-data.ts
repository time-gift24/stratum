"use client"

/**
 * 设置区列表数据：fetch + 归一化的唯一实现（Provider 前端过滤分页，
 * Model 使用 DB-backed 兼容 catalog 的单次投影 + Provider 侧推 hasResources），供 SettingsList 加载
 * 与 SettingsNav 悬停预取共用，避免两套逻辑漂移。
 */

import { studioApi } from "@/features/studio-management/client"
import { readPageCache, writePageCache } from "@/lib/page-cache"
import type {
  ManagedModelSummary,
  PageEnvelope,
  ProviderView,
} from "@/lib/stratum/api"

export const SETTINGS_PER_PAGE = 12

export type SettingsKind = "providers" | "models"

export type SettingsPageData = {
  result: PageEnvelope<ProviderView> | PageEnvelope<ManagedModelSummary>
  hasResources: boolean
}

const pendingSettingsPages = new Map<string, Promise<SettingsPageData>>()

async function loadSettingsPage(
  kind: SettingsKind,
  page: number,
  query: string
): Promise<SettingsPageData> {
  if (kind === "providers") {
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
  const [modelPage, providerPage] = await Promise.all([
    studioApi.listManagedModels({
      page,
      perPage: SETTINGS_PER_PAGE,
      search: query,
    }),
    studioApi.listProviders({ page: 1, perPage: 100 }),
  ])
  return {
    result: modelPage,
    hasResources: providerPage.data.some(
      (provider) => provider.models_count > 0
    ),
  }
}

/** Shares one in-flight request across hover, focus, and the destination list. */
export function fetchSettingsPage(
  kind: SettingsKind,
  page: number,
  query: string
): Promise<SettingsPageData> {
  const key = `${kind}:${page}:${query}`
  const pending = pendingSettingsPages.get(key)
  if (pending) return pending

  const request = loadSettingsPage(kind, page, query)
  pendingSettingsPages.set(key, request)
  void request.then(
    () => {
      if (pendingSettingsPages.get(key) === request)
        pendingSettingsPages.delete(key)
    },
    () => {
      if (pendingSettingsPages.get(key) === request)
        pendingSettingsPages.delete(key)
    }
  )
  return request
}

/**
 * 悬停/聚焦预取页签默认视图（第一页、无搜索），写入页面缓存；
 * 命中缓存或失败都静默——失败则切换时正常走加载态。
 */
export function prefetchSettingsLanding(kind: SettingsKind): void {
  const cacheKey = `studio-settings:${kind}:1:`
  if (readPageCache(cacheKey) !== null) return
  void fetchSettingsPage(kind, 1, "")
    .then((data) => writePageCache(cacheKey, data))
    .catch(() => {})
}
