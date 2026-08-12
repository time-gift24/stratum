import { describe, expect, it } from "vitest"

import { presentConversationError } from "./error-notice"
import { ApiError } from "@/lib/stratum/api"

describe("presentConversationError", () => {
  it("describes a backend outage without turning it into message content", () => {
    expect(
      presentConversationError(
        "ready",
        new ApiError("store_unavailable", 503, "database unavailable")
      )
    ).toEqual({
      title: "暂时无法连接到 Stratum 后端",
      description: "服务恢复后会自动同步，也可以直接重试刚才的操作。",
    })
  })

  it("distinguishes a missing runtime from a connection outage", () => {
    expect(
      presentConversationError(
        "missing",
        new ApiError("agent_runtime_not_found", 404, "runtime does not exist")
      )
    ).toEqual({
      title: "会话无法加载",
      description: "该会话可能已删除，或属于另一个 Stratum 运行环境。",
    })
  })

  it("gives local input errors their own actionable wording", () => {
    expect(
      presentConversationError(
        "empty",
        new ApiError("invalid_input", 400, "message is required")
      )
    ).toEqual({
      title: "内容无法发送",
      description: "请先输入消息，再重新发送。",
    })
  })
})
