"use client"

/**
 * 客户端页面数据缓存（SWR 语义）：跨路由切换保留最近一次成功响应，
 * 重访时先渲染缓存、后台刷新后替换，骨架只在真正的冷启动出现。
 * 只服务 client 组件的页面级读取；写操作后按前缀失效。
 */

const store = new Map<string, unknown>()

export function readPageCache<T>(key: string): T | null {
  return (store.get(key) as T | undefined) ?? null
}

export function writePageCache(key: string, value: unknown): void {
  store.set(key, value)
}

/** 按前缀失效；不传前缀清空全部（删除类操作后用）。 */
export function invalidatePageCache(prefix?: string): void {
  if (prefix === undefined) {
    store.clear()
    return
  }
  for (const key of store.keys()) {
    if (key.startsWith(prefix)) store.delete(key)
  }
}
