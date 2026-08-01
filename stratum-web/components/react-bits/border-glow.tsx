"use client"

import {
  useEffect,
  useMemo,
  useRef,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { cn } from "@/lib/utils"

import styles from "./glow-border.module.css"

/**
 * BorderGlow —— 追光渐变边框卡片（reactbits BorderGlow-JS-CSS 改造，TSX）。
 * 指针接近边缘时彩色 mesh 边框与外发光随角度/接近度浮现。
 *
 * 与原版的差异（我们的组件标准）：
 * - token 化：底色回落 --card、边框 --border，渐变默认色取自 port/primary 家族
 * - pointermove 经 rAF 节流，每帧最多一次样式写入；卸载取消帧回调
 * - 入场 sweep 用 GSAP timeline（useGSAP context 自动清理），替代手搓 rAF 链
 * - Safari mask-composite 前缀回退与 @supports 护栏在 module.css 内
 */

export interface BorderGlowProps {
  children: ReactNode
  className?: string
  /** 外发光开始响应的边缘接近度阈值（0-100） */
  edgeSensitivity?: number
  /** 外发光颜色，HSL 三元组字符串："h s l" */
  glowColor?: string
  /** 卡片底色；不传则跟随 --card token */
  backgroundColor?: string
  borderRadius?: number
  glowRadius?: number
  glowIntensity?: number
  /** 追光锥形张角（%） */
  coneSpread?: number
  /** 挂载时播放一圈扫光 */
  animated?: boolean
  /** 全线段激活：整圈边框与外发光常亮（聚焦等持续态） */
  active?: boolean
  /** 去掉多层投影（嵌套在自重较小的场景） */
  flat?: boolean
  /** mesh 渐变取色（映射到 7 个固定位置） */
  colors?: string[]
  /** 边缘填色不透明度上限 */
  fillOpacity?: number
}

function parseHSL(hslStr: string) {
  const match = hslStr.match(/([\d.]+)\s*([\d.]+)%?\s*([\d.]+)%?/)
  if (!match) return { h: 40, s: 80, l: 80 }
  return { h: parseFloat(match[1]), s: parseFloat(match[2]), l: parseFloat(match[3]) }
}

const GLOW_OPACITIES = [100, 60, 50, 40, 30, 20, 10] as const
const GLOW_KEYS = ["", "-60", "-50", "-40", "-30", "-20", "-10"] as const

function buildGlowVars(glowColor: string, intensity: number) {
  const { h, s, l } = parseHSL(glowColor)
  const vars: Record<string, string> = {}
  for (let i = 0; i < GLOW_OPACITIES.length; i++) {
    vars[`--glow-color${GLOW_KEYS[i]}`] =
      `hsl(${h}deg ${s}% ${l}% / ${Math.min(GLOW_OPACITIES[i] * intensity, 100)}%)`
  }
  return vars
}

const GRADIENT_POSITIONS = [
  "80% 55%",
  "69% 34%",
  "8% 6%",
  "41% 38%",
  "86% 85%",
  "82% 18%",
  "51% 4%",
] as const
const GRADIENT_KEYS = [
  "--gradient-one",
  "--gradient-two",
  "--gradient-three",
  "--gradient-four",
  "--gradient-five",
  "--gradient-six",
  "--gradient-seven",
] as const
const COLOR_MAP = [0, 1, 2, 0, 1, 2, 1] as const

function buildGradientVars(colors: string[]) {
  const vars: Record<string, string> = {}
  for (let i = 0; i < GRADIENT_KEYS.length; i++) {
    const color = colors[Math.min(COLOR_MAP[i], colors.length - 1)]
    vars[GRADIENT_KEYS[i]] =
      `radial-gradient(at ${GRADIENT_POSITIONS[i]}, ${color} 0px, transparent 50%)`
  }
  vars["--gradient-base"] = `linear-gradient(${colors[0]} 0 100%)`
  return vars
}

function getEdgeProximity(rect: DOMRect, x: number, y: number) {
  const cx = rect.width / 2
  const cy = rect.height / 2
  const dx = x - cx
  const dy = y - cy
  const kx = dx !== 0 ? cx / Math.abs(dx) : Infinity
  const ky = dy !== 0 ? cy / Math.abs(dy) : Infinity
  return Math.min(Math.max(1 / Math.min(kx, ky), 0), 1)
}

function getCursorAngle(rect: DOMRect, x: number, y: number) {
  const dx = x - rect.width / 2
  const dy = y - rect.height / 2
  if (dx === 0 && dy === 0) return 0
  const degrees = (Math.atan2(dy, dx) * 180) / Math.PI + 90
  return degrees < 0 ? degrees + 360 : degrees
}

/** 默认 mesh 配色：port 家族三色（模块级常量，保证 useMemo 依赖稳定） */
const DEFAULT_COLORS = ["var(--port-image)", "var(--port-negative)", "var(--primary)"]

export function BorderGlow({
  children,
  className,
  edgeSensitivity = 30,
  glowColor = "152 55 62",
  backgroundColor,
  borderRadius = 28,
  glowRadius = 40,
  glowIntensity = 1.0,
  coneSpread = 25,
  animated = false,
  active = false,
  flat = false,
  colors = DEFAULT_COLORS,
  fillOpacity = 0.5,
}: BorderGlowProps) {
  const cardRef = useRef<HTMLDivElement>(null)
  const frame = useRef(0)
  const pendingPoint = useRef<{ x: number; y: number } | null>(null)

  // rAF 节流：每帧最多写一次样式，卸载时取消挂起的帧
  useEffect(() => () => cancelAnimationFrame(frame.current), [])

  const applyFrame = () => {
    frame.current = 0
    const point = pendingPoint.current
    const card = cardRef.current
    if (!point || !card) return
    const rect = card.getBoundingClientRect()
    const x = point.x - rect.left
    const y = point.y - rect.top
    card.style.setProperty(
      "--edge-proximity",
      `${(getEdgeProximity(rect, x, y) * 100).toFixed(3)}`
    )
    card.style.setProperty(
      "--cursor-angle",
      `${getCursorAngle(rect, x, y).toFixed(3)}deg`
    )
  }

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    pendingPoint.current = { x: e.clientX, y: e.clientY }
    if (frame.current === 0) {
      frame.current = requestAnimationFrame(applyFrame)
    }
  }

  // 入场扫光：GSAP timeline，useGSAP context 在卸载/依赖变化时自动清理
  useGSAP(
    () => {
      if (!animated) return
      const card = cardRef.current
      if (!card) return

      const state = { angle: 110, proximity: 0 }
      const apply = () => {
        card.style.setProperty("--cursor-angle", `${state.angle}deg`)
        card.style.setProperty("--edge-proximity", `${state.proximity}`)
      }

      card.classList.add(styles.sweepActive)
      const tl = gsap.timeline()
      tl.to(state, {
        proximity: 100,
        duration: 0.5,
        ease: "power3.out",
        onUpdate: apply,
      }, 0)
        .to(state, {
          angle: 287.5,
          duration: 1.5,
          ease: "power3.in",
          onUpdate: apply,
        }, 0)
        .to(state, {
          angle: 465,
          duration: 2.25,
          ease: "power3.out",
          onUpdate: apply,
        }, 1.5)
        .to(state, {
          proximity: 0,
          duration: 1.5,
          ease: "power3.in",
          onUpdate: apply,
        }, 2.5)

      return () => card.classList.remove(styles.sweepActive)
    },
    { scope: cardRef, dependencies: [animated] }
  )

  const style = useMemo(
    () =>
      ({
        "--card-bg": backgroundColor,
        "--edge-sensitivity": edgeSensitivity,
        "--border-radius": `${borderRadius}px`,
        "--glow-padding": `${glowRadius}px`,
        "--cone-spread": coneSpread,
        "--fill-opacity": fillOpacity,
        ...buildGlowVars(glowColor, glowIntensity),
        ...buildGradientVars(colors),
      }) as CSSProperties,
    [
      backgroundColor,
      edgeSensitivity,
      borderRadius,
      glowRadius,
      coneSpread,
      fillOpacity,
      glowColor,
      glowIntensity,
      colors,
    ]
  )

  return (
    <div
      ref={cardRef}
      onPointerMove={handlePointerMove}
      className={cn(
        styles.card,
        active && styles.active,
        flat && styles.flat,
        className
      )}
      style={style}
    >
      <span className={styles.edgeLight} />
      <div className={styles.inner}>{children}</div>
    </div>
  )
}

export default BorderGlow
