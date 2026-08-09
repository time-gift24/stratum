import {
  ApiError,
  type StratumApi,
} from "@/lib/stratum/api"
import {
  ontologyDocumentsEqual,
  type OntologyEditorAction,
} from "@/features/ontology-editor/reducer"
import type { OntologyDocument } from "@/features/ontology-editor/types"

// 保存副作用编排（对应 features/agent-conversation/recovery.ts 的注入式风格）：
// hook 负责快照与 dispatch save_started，这里负责 PUT 与各类结果的后续动作。
export type OntologySaveDependencies = {
  api: Pick<StratumApi, "replaceOntology" | "getOntology">
  dispatch(action: OntologyEditorAction): void
}

export type OntologySaveResult =
  | { outcome: "saved"; etag: string }
  | { outcome: "conflict" }
  | { outcome: "invalid" }
  | { outcome: "failed" }

export async function attemptOntologySave(
  dependencies: OntologySaveDependencies,
  input: { ontologyId: string; document: OntologyDocument; baseEtag: string }
): Promise<OntologySaveResult> {
  try {
    const result = await dependencies.api.replaceOntology(
      input.ontologyId,
      input.document,
      input.baseEtag
    )
    dependencies.dispatch({ type: "save_succeeded", etag: result.etag })
    return { outcome: "saved", etag: result.etag }
  } catch (error) {
    if (error instanceof ApiError) {
      if (error.status === 412) {
        // 412：重读最新资源交用户调和，绝不静默换新 ETag 重试
        try {
          const remote = await dependencies.api.getOntology(input.ontologyId)
          dependencies.dispatch({
            type: "save_conflict",
            remote: { document: remote.document, etag: remote.etag },
          })
          return { outcome: "conflict" }
        } catch (readError) {
          dependencies.dispatch({
            type: "save_failed",
            error: toApiError(readError),
          })
          return { outcome: "failed" }
        }
      }
      if (error.status === 422) {
        dependencies.dispatch({
          type: "save_invalid",
          violations: error.violations ?? [],
        })
        return { outcome: "invalid" }
      }
      // 其它确定性 HTTP 错误（409/413/428/5xx…）：直接失败，不重读
      dependencies.dispatch({ type: "save_failed", error })
      return { outcome: "failed" }
    }

    // 网络错误/超时：响应可能已丢失，先重读判断 in_flight 是否已提交
    try {
      const remote = await dependencies.api.getOntology(input.ontologyId)
      if (ontologyDocumentsEqual(remote.document, input.document)) {
        dependencies.dispatch({ type: "save_succeeded", etag: remote.etag })
        return { outcome: "saved", etag: remote.etag }
      }
      dependencies.dispatch({
        type: "save_failed",
        error: new ApiError(
          "save_unconfirmed",
          0,
          "save result is unknown; remote differs from the in-flight document"
        ),
      })
    } catch (readError) {
      dependencies.dispatch({ type: "save_failed", error: toApiError(readError) })
    }
    return { outcome: "failed" }
  }
}

function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error
  return new ApiError(
    "connection_error",
    0,
    error instanceof Error ? error.message : "connection failed"
  )
}
