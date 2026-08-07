"use client"

import { Excalidraw } from "@excalidraw/excalidraw"
import { useTheme } from "next-themes"

// Excalidraw 样式表随本模块动态 chunk 懒加载（144K，不进首屏 CSS）
import "@excalidraw/excalidraw/index.css"

/**
 * WhiteboardEditor —— 白板页的完整 Excalidraw 编辑器（经
 * whiteboard-workspace 动态导入）。默认可编辑，主题跟随 next-themes。
 * 画布透明：透出外层页面 --background，与整站底色零色差；
 * Excalidraw 自带的主题切换与画布底色入口隐藏，统一跟随站点主题。
 */
export function WhiteboardEditor() {
  const { resolvedTheme } = useTheme()
  return (
    <Excalidraw
      theme={resolvedTheme === "dark" ? "dark" : "light"}
      initialData={{ appState: { viewBackgroundColor: "transparent" } }}
      UIOptions={{
        canvasActions: {
          changeViewBackgroundColor: false,
          toggleTheme: false,
        },
      }}
    />
  )
}
