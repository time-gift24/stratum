"use client"

import dynamic from "next/dynamic"

import styles from "@/components/stratum/styles/excalidraw-theme.module.css"
import { cn } from "@/lib/utils"

/**
 * WhiteboardWorkspace —— 白板页外壳：动态加载 Excalidraw 编辑器
 * （SSR 关闭，库与样式表不进首屏），加载中显示同形状骨架。
 * 容器挂主题映射（excalidraw-theme.module）+ 页面底色，
 * 透明画布透出 --background，与整站零色差。
 */

function WhiteboardSkeleton() {
  return (
    <div
      aria-hidden
      className="h-full w-full animate-pulse bg-muted/50 motion-reduce:animate-none"
    />
  )
}

const WhiteboardEditor = dynamic(
  () =>
    import("@/components/stratum/excalidraw/whiteboard-editor").then(
      (mod) => mod.WhiteboardEditor
    ),
  { ssr: false, loading: () => <WhiteboardSkeleton /> }
)

export function WhiteboardWorkspace() {
  return (
    <div
      data-slot="whiteboard-workspace"
      className={cn("h-full w-full bg-background", styles.theme)}
    >
      <WhiteboardEditor />
    </div>
  )
}
