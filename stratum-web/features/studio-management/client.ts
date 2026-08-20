import {
  ApiError,
  createStratumApi,
  STRATUM_API_BASE_URL,
} from "@/lib/stratum/api"

export const studioApi = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })

/** 只展示 API 的公开错误文案；网络/协议异常统一收敛到调用方提供的安全提示。 */
export function safeStudioErrorMessage(
  error: unknown,
  fallback: string
): string {
  return error instanceof ApiError ? error.message : fallback
}
