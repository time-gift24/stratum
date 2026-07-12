const LOCAL_WYSE_API_BASE_URL = "http://127.0.0.1:18080"

export function apiConfiguration(): { baseUrl: string } {
  return { baseUrl: LOCAL_WYSE_API_BASE_URL }
}
