"use client"

import { Excalidraw } from "@excalidraw/excalidraw"
import type { ExcalidrawProps } from "@excalidraw/excalidraw/types"
import { useTheme } from "next-themes"

// Excalidraw 样式表随本模块动态 chunk 懒加载（144K，不进首屏 CSS）
import "@excalidraw/excalidraw/index.css"

/** 静态 props 提升为模块级常量，避免每次渲染新引用 */
const INITIAL_DATA: ExcalidrawProps["initialData"] = {
  appState: { viewBackgroundColor: "transparent" },
}

const EDITOR_UI_OPTIONS: ExcalidrawProps["UIOptions"] = {
  canvasActions: {
    changeViewBackgroundColor: false,
    toggleTheme: false,
  },
}

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
      initialData={INITIAL_DATA}
      UIOptions={EDITOR_UI_OPTIONS}
    />
  )
}
