"use client"

import { useEffect, useRef, useState } from "react"
import gsap from "gsap"

/**
 * useSmoothText —— 流式文本的水流式平滑呈现（纯渲染端，不动数据层）。
 *
 * active（streaming 中）：用 gsap.ticker（GSAP 的 rAF 循环，与仓库动效体系
 * 共用同一帧循环）让显示文本匀速追赶 target。步长自适应：
 * `max(1, ceil(backlog / 4))`——爆发式 chunk 按比例加速追平（永不显著滞后），
 * 零星到达则 1 字符/帧缓慢流出，保留流动感（ease-out 尾巴）。
 *
 * snap 保证：active 结束（streaming → done/failed）或 prefers-reduced-motion
 * 时直接返回 target 全文，绝不让队列滞留。target 变化只更新追赶目标；
 * 目标不是当前显示的前缀（换消息/重置）时立即对齐，绝不回放。
 *
 * 驱动选型 gsap.ticker 而非自起 rAF：仓库约定动效统一 GSAP，ticker 与页面
 * 其它 GSAP 动画共享同一 rAF 循环（少一个循环），API 等价。
 */
export function useSmoothText(target: string, active: boolean): string {
  const [display, setDisplay] = useState("")
  const shownRef = useRef("")
  const targetRef = useRef(target)
  const [reduce] = useState(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches
  )

  // ticker 读取最新目标（ref 只在 effect 中写）
  useEffect(() => {
    targetRef.current = target
  }, [target])

  useEffect(() => {
    if (reduce || !active) {
      shownRef.current = targetRef.current
      return
    }
    const tick = () => {
      const goal = targetRef.current
      const shown = shownRef.current
      if (shown === goal) return
      if (!goal.startsWith(shown)) {
        shownRef.current = goal
        setDisplay(goal)
        return
      }
      const backlog = goal.length - shown.length
      const step = Math.max(1, Math.ceil(backlog / 4))
      const next = goal.slice(0, shown.length + step)
      shownRef.current = next
      setDisplay(next)
    }
    gsap.ticker.add(tick)
    return () => {
      gsap.ticker.remove(tick)
    }
  }, [active, reduce])

  if (reduce || !active) return target
  return display
}
