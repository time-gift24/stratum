export const DEFAULT_STUDIO_RETURN = "/studio"

export function safeStudioReturn(value: string | null): string {
  if (value === null) return DEFAULT_STUDIO_RETURN
  try {
    const url = new URL(value, "http://stratum.local")
    if (url.origin !== "http://stratum.local" || url.pathname !== "/studio")
      return DEFAULT_STUDIO_RETURN
    return `${url.pathname}${url.search}`
  } catch {
    return DEFAULT_STUDIO_RETURN
  }
}

export function withStudioReturn(path: string, returnTo: string): string {
  const [pathname, query = ""] = path.split("?", 2)
  const params = new URLSearchParams(query)
  params.set("returnTo", safeStudioReturn(returnTo))
  return `${pathname}?${params}`
}
