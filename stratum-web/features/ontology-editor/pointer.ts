// RFC 6901 JSON Pointer 解析为段数组：`~1` → `/`，`~0` → `~`（顺序固定，先 ~1 后 ~0）。
// 返回 null 表示指针格式非法（非空串必须以 `/` 开头，且 `~` 后只能跟 0 或 1）。

const INVALID_ESCAPE = /~(?:[^01]|$)/
const ESCAPED_SLASH = /~1/g
const ESCAPED_TILDE = /~0/g

export function parseJsonPointer(pointer: string): readonly string[] | null {
  if (pointer === "") return []
  if (!pointer.startsWith("/")) return null

  const segments = pointer.slice(1).split("/")
  const unescaped: string[] = []
  for (const segment of segments) {
    if (INVALID_ESCAPE.test(segment)) return null
    unescaped.push(segment.replace(ESCAPED_SLASH, "/").replace(ESCAPED_TILDE, "~"))
  }
  return unescaped
}
