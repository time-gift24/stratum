/**
 * motion —— 全站动效尺度（唯一事实源）。
 *
 * 时长三档：
 * - fast 0.3s：退场、纯淡入淡出（内容越轻，越快）
 * - base 0.4s：进场、高度手风琴
 * - slow 0.55s：大位移空间移动（composer 中心 ⇄ 底部）
 *
 * 缓动两条：
 * - enter（expo.out）：所有"出现"——浮层、页面、手风琴、淡入
 * - exit（power2.in）：所有"消失"——退场、收起、页面滑出
 *
 * 例外：lib/scroll-to.ts 的锚点滚动按距离自适应（0.5–1.2s power3.inOut），
 * 属于滚动行程而非进/退场，不纳入本尺度；react-bits 底稿（site-nav、
 * border-glow）只读，其内部时长不强制对齐。
 */

export const MOTION_DURATION = {
  fast: 0.3,
  base: 0.4,
  slow: 0.55,
} as const

export const MOTION_EASE = {
  enter: "expo.out",
  exit: "power2.in",
} as const

export const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)"

export function prefersReducedMotion(): boolean {
  return window.matchMedia(REDUCED_MOTION_QUERY).matches
}

/** 动效时长：prefers-reduced-motion 时瞬时（0），调用处不再各自写三元 */
export function motionDuration(seconds: number): number {
  return prefersReducedMotion() ? 0 : seconds
}
