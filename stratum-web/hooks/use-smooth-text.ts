"use client"

import { useEffect, useRef, useState, useSyncExternalStore } from "react"
import gsap from "gsap"

import { REDUCED_MOTION_QUERY } from "@/lib/motion"

/**
 * useSmoothText —— 流式文本的水流式平滑呈现（纯渲染端，不动数据层）。
 *
 * active（streaming 中）：用 gsap.ticker（GSAP 的 rAF 循环，与仓库动效体系
 * 共用同一帧循环）让显示文本匀速追赶 target。步长自适应：
 * `max(1, ceil(backlog / 4))`——爆发式 chunk 按比例加速追平（永不显著滞后），
 * 零星到达则 1 字符/帧缓慢流出，保留流动感（ease-out 尾巴）。
 * setDisplay 隔帧节流（shownRef 每帧照推，距上次 flush ≥ 2 帧或追平时才提交），
 * 下游 Streamdown 重解析开销减半。
 *
 * snap 保证：active 结束（streaming → done/failed）或 prefers-reduced-motion
 * 时直接返回 target 全文，绝不让队列滞留。target 变化只更新追赶目标；
 * 目标不是当前显示的前缀（换消息/重置）时立即对齐，绝不回放。
 * reduced-motion 用 useSyncExternalStore 订阅 matchMedia change（server
 * snapshot 恒 false），运行期切换即时生效、无 hydration 窗口。
 *
 * 驱动选型 gsap.ticker 而非自起 rAF：仓库约定动效统一 GSAP，ticker 与页面
 * 其它 GSAP 动画共享同一 rAF 循环（少一个循环），API 等价。
 */

function subscribeReducedMotion(onChange: () => void) {
  const query = window.matchMedia(REDUCED_MOTION_QUERY)
  query.addEventListener("change", onChange)
  return () => query.removeEventListener("change", onChange)
}

export function useSmoothText(target: string, active: boolean): string {
  const [display, setDisplay] = useState("")
  const shownRef = useRef("")
  const targetRef = useRef(target)
  const reduce = useSyncExternalStore(
    subscribeReducedMotion,
    () => window.matchMedia(REDUCED_MOTION_QUERY).matches,
    () => false
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
    // 距上次 flush 的帧数（隔帧节流计数器，effect 闭包内私有）
    let framesSinceFlush = 0
    const flush = (next: string) => {
      framesSinceFlush = 0
      setDisplay(next)
    }
    const tick = () => {
      const goal = targetRef.current
      const shown = shownRef.current
      if (shown === goal) return
      if (!goal.startsWith(shown)) {
        shownRef.current = goal
        flush(goal)
        return
      }
      const backlog = goal.length - shown.length
      const step = Math.max(1, Math.ceil(backlog / 4))
      const next = goal.slice(0, shown.length + step)
      shownRef.current = next
      framesSinceFlush += 1
      // 追平立即 flush（结尾不滞留）；其余每 2 帧提交一次
      if (next === goal || framesSinceFlush >= 2) flush(next)
    }
    gsap.ticker.add(tick)
    return () => {
      gsap.ticker.remove(tick)
    }
  }, [active, reduce])

  if (reduce || !active) return target
  return display
}
