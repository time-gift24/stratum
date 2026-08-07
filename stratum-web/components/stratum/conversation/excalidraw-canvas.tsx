"use client"

import { Excalidraw } from "@excalidraw/excalidraw"
import type { ExcalidrawInitialDataState } from "@excalidraw/excalidraw/types"
import { useTheme } from "next-themes"

// Excalidraw 样式表随本模块动态 chunk 懒加载（144K，不进首屏 CSS）
import "@excalidraw/excalidraw/index.css"

import type { ExcalidrawScene } from "@/components/stratum/conversation/excalidraw-result"

/**
 * ExcalidrawCanvas —— 只读白板的实际渲染（经 excalidraw-result 动态导入）。
 * view mode 只读；主题跟随 next-themes；canvasActions 隐藏编辑入口。
 * 画布透明：透出卡片 --card 底色（场景自带底色被覆盖，保证零色差）。
 */
export function ExcalidrawCanvas({ scene }: { scene: ExcalidrawScene }) {
  const { resolvedTheme } = useTheme()

  // elements 只校验到"是数组"，单项结构信任后端工具产出
  const initialData = {
    elements: scene.elements,
    appState: { ...scene.appState, viewBackgroundColor: "transparent" },
  } as ExcalidrawInitialDataState

  return (
    <Excalidraw
      initialData={initialData}
      viewModeEnabled
      zenModeEnabled={false}
      gridModeEnabled={false}
      theme={resolvedTheme === "dark" ? "dark" : "light"}
      UIOptions={{
        canvasActions: {
          changeViewBackgroundColor: false,
          clearCanvas: false,
          export: false,
          loadScene: false,
          saveAsImage: false,
          saveToActiveFile: false,
          toggleTheme: false,
        },
      }}
    />
  )
}
