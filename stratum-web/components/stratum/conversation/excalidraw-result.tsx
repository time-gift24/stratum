"use client"

import { Component, useMemo, type ReactNode } from "react"
import dynamic from "next/dynamic"

import styles from "@/components/stratum/styles/excalidraw-theme.module.css"
import { cn } from "@/lib/utils"

/**
 * ExcalidrawResult —— excalidraw_render 工具结果的只读白板预览。
 * 渐进式透明：作为工具调用块展开区的结果内容内嵌展示，view mode 只读。
 * 库与样式表经 next/dynamic 动态加载（SSR 关闭），折叠状态下不触发加载；
 * 加载中显示同形状骨架；形状校验失败或加载/渲染失败返回 null，
 * 由调用方回退到原始 JSON 展示。
 *
 * 实际渲染在 excalidraw-canvas.tsx（携带 Excalidraw 样式表），
 * 与本文件分离以保证 CSS 随动态 chunk 懒加载。
 */

/** 最小 scene 形状（design D2 契约）：JSON 对象且 elements 为数组 */
export type ExcalidrawScene = {
  elements: unknown[]
  appState?: Record<string, unknown>
}

/** 解析并校验工具结果文本；不合法时返回 null */
export function parseExcalidrawScene(text: string): ExcalidrawScene | null {
  try {
    const value: unknown = JSON.parse(text)
    if (typeof value !== "object" || value === null) return null
    const elements = (value as { elements?: unknown }).elements
    if (!Array.isArray(elements)) return null
    const appState = (value as { appState?: unknown }).appState
    return {
      elements,
      appState:
        typeof appState === "object" && appState !== null
          ? (appState as Record<string, unknown>)
          : undefined,
    }
  } catch {
    return null
  }
}

/** 与白板卡片同形状同高度的骨架，避免加载完成时布局跳动 */
function WhiteboardSkeleton() {
  return (
    <div
      aria-hidden
      className="h-80 w-full animate-pulse rounded-xl bg-muted/50 motion-reduce:animate-none"
    />
  )
}

const ExcalidrawCanvas = dynamic(
  () =>
    import("@/components/stratum/conversation/excalidraw-canvas").then(
      (mod) => mod.ExcalidrawCanvas
    ),
  { ssr: false, loading: () => <WhiteboardSkeleton /> }
)

/** 动态导入或渲染失败时静默降级为 null，由调用方回退原始 JSON */
class ExcalidrawErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false }

  static getDerivedStateFromError() {
    return { failed: true }
  }

  render() {
    return this.state.failed ? null : this.props.children
  }
}

export function ExcalidrawResult({
  sceneText,
  className,
}: {
  /** 工具结果 JSON 文本；形状不合法时渲染 null（调用方回退原始 JSON） */
  sceneText: string
  className?: string
}) {
  const scene = useMemo(() => parseExcalidrawScene(sceneText), [sceneText])
  if (scene === null) return null

  return (
    <div
      data-slot="excalidraw-result"
      className={cn(
        "h-80 overflow-hidden rounded-xl border border-border bg-card",
        styles.theme,
        className
      )}
    >
      <ExcalidrawErrorBoundary>
        <ExcalidrawCanvas scene={scene} />
      </ExcalidrawErrorBoundary>
    </div>
  )
}
